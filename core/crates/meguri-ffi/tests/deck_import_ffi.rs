//! Deck package import through its real exports (`package/apkg-import`,
//! `package/transfer-sheets` X2): classify, inspect, the stored options,
//! apply, the log's pages, undo, the session's end, the refusals a file or
//! the device gives, and a cancel in each phase that can be cancelled.
//!
//! The package is the open collection written by the production package
//! writer under a deck package's name (`test_write_deck_package`).
use meguri_ffi::{
    CoreEvent, CoreEventKind, CoreEventListener, JobHandle, JobOutcome,
    collection::{self, CollectionLocation, DiskCapacity, DiskSpaceProvider, HostClock},
    deck_import::*,
    lifecycle::close_collection,
    notes::{AddNoteRequest, UpdateNoteRequest, add_note, update_note},
    notetype_structure::{
        NotetypeConfirmedLosses, NotetypeStructureDraft, StructureDraftField, StructureDraftTemplate,
        start_notetype_structure_save,
    },
    notetypes::{notetype_names, read_notetype},
    review_session::{ReviewUndoScope, ReviewUndoStatus},
    test_hooks::test_write_deck_package,
    undo::{peek_undo, start_undo},
};
use std::{
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering},
        mpsc,
    },
    time::Duration,
};

/// A pause a test arms: the next call that passes it tells the test and
/// waits for its answer. The clock and the space provider pass it, which
/// the import calls at known points of its work.
struct Gate {
    armed: AtomicBool,
    hit: mpsc::Sender<()>,
    go: Mutex<mpsc::Receiver<()>>,
}
impl Gate {
    fn new() -> (Arc<Gate>, mpsc::Receiver<()>, mpsc::Sender<()>) {
        let (hit, hits) = mpsc::channel();
        let (resume, go) = mpsc::channel();
        (Arc::new(Gate { armed: AtomicBool::new(false), hit, go: Mutex::new(go) }), hits, resume)
    }
    fn pass(&self) {
        if self.armed.swap(false, Ordering::SeqCst) {
            let _ = self.hit.send(());
            let _ = self.go.lock().unwrap().recv_timeout(Duration::from_secs(20));
        }
    }
}
struct Clock {
    now: AtomicI64,
    gate: Arc<Gate>,
}
impl HostClock for Clock {
    fn now_ms(&self) -> i64 {
        self.gate.pass();
        self.now.load(Ordering::SeqCst)
    }
    fn offset_from_utc(&self, _: i64) -> i32 {
        0
    }
    fn zone_id(&self) -> String {
        "UTC".into()
    }
}
struct Space {
    bytes: AtomicU64,
    gate: Arc<Gate>,
}
impl DiskSpaceProvider for Space {
    fn available_bytes(&self, _: String) -> DiskCapacity {
        self.gate.pass();
        DiskCapacity::Available { bytes: self.bytes.load(Ordering::SeqCst) }
    }
}
struct Listener(mpsc::Sender<CoreEvent>);
impl CoreEventListener for Listener {
    fn on_event(&self, event: CoreEvent) {
        let _ = self.0.send(event);
    }
}

struct Device {
    events: mpsc::Receiver<CoreEvent>,
    clock: Arc<Clock>,
    space: Arc<Space>,
    clock_hits: mpsc::Receiver<()>,
    clock_go: mpsc::Sender<()>,
    space_hits: mpsc::Receiver<()>,
    space_go: mpsc::Sender<()>,
    directory: PathBuf,
}
impl Device {
    fn finish(&self, id: u64) -> JobOutcome {
        loop {
            let event = self.events.recv_timeout(Duration::from_secs(60)).unwrap();
            if let CoreEventKind::JobFinished { id: got, outcome } = event.kind
                && got == id
            {
                return outcome;
            }
        }
    }
    fn staging(&self) -> usize {
        std::fs::read_dir(&self.directory)
            .unwrap()
            .filter(|entry| entry.as_ref().unwrap().file_name().to_string_lossy().starts_with(".meguri-stage-"))
            .count()
    }
    fn media(&self) -> usize {
        std::fs::read_dir(self.directory.join("collection.media")).map_or(0, |entries| entries.count())
    }
    fn backups(&self) -> usize {
        std::fs::read_dir(self.directory.parent().unwrap().join("backups")).map_or(0, |entries| entries.count())
    }
}

const NOW: i64 = 1_894_701_600_000;

/// Opens a new collection under `root`. Every collection of these tests is
/// made with the same seed, so they hold the same stock note types.
fn open(root: &Path) -> Device {
    let directory = root.join("collection");
    let backups = root.join("backups");
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::create_dir_all(&backups).unwrap();
    meguri_engine::bootstrap::create(
        &directory.join("collection.anki2"),
        &meguri_model::clock::FixedClock::new(NOW, meguri_model::clock::UtcOffset::UTC),
        &meguri_model::rng::SeededRandom::new(7),
    )
    .unwrap();
    let (tx, events) = mpsc::channel();
    meguri_ffi::events::set_event_listener(Arc::new(Listener(tx))).unwrap();
    let (clock_gate, clock_hits, clock_go) = Gate::new();
    let (space_gate, space_hits, space_go) = Gate::new();
    let clock = Arc::new(Clock { now: AtomicI64::new(NOW), gate: clock_gate });
    let space = Arc::new(Space { bytes: AtomicU64::new(u64::MAX), gate: space_gate });
    let handle = collection::start_open(
        CollectionLocation {
            manual_transfer_directory: root.join("ManualTransfers").to_string_lossy().into_owned(),
            collection_directory: directory.to_str().unwrap().into(),
            backups_directory: backups.to_str().unwrap().into(),
            app_group_snapshot: root.join("widget.snapshot").to_str().unwrap().into(),
        },
        clock.clone(),
        space.clone(),
        7,
    )
    .unwrap();
    let device = Device { events, clock, space, clock_hits, clock_go, space_hits, space_go, directory };
    assert_eq!(device.finish(handle.id), JobOutcome::Succeeded);
    device
}

fn basic() -> i64 {
    notetype_names().unwrap().into_iter().find(|notetype| notetype.name == "Basic").unwrap().id
}
fn note(guid: &str, front: &str) -> i64 {
    add_note(AddNoteRequest {
        notetype_id: basic(),
        fields: vec![front.into(), "back".into()],
        tags: Vec::new(),
        deck_id: 1,
        guid: Some(guid.into()),
        update_add_defaults: false,
    })
    .unwrap()
    .id
}

/// A package of two notes under `root`, then a new, empty target collection.
/// `later` holds the same notes with the first one edited afterwards.
fn package_and_target(root: &Path) -> (PathBuf, PathBuf, Device) {
    let source = open(&root.join("source"));
    let first = note("g-first", "sun");
    note("g-second", "moon");
    let package = root.join("deck.apkg");
    test_write_deck_package(package.to_str().unwrap().into()).unwrap();
    source.clock.now.store(NOW + 90_000, Ordering::SeqCst);
    update_note(UpdateNoteRequest { id: first, fields: vec!["sun-zq2".into(), "back".into()], tags: Vec::new() }).unwrap();
    let later = root.join("later.apkg");
    test_write_deck_package(later.to_str().unwrap().into()).unwrap();
    close_collection().unwrap();
    drop(source);
    (package, later, open(&root.join("target")))
}

fn inspect(device: &Device, package: &Path) -> (JobOutcome, DeckImportInspectOutcome) {
    let name = package.file_name().unwrap().to_str().unwrap().to_owned();
    let started = start_deck_import_inspect(package.to_str().unwrap().into(), name).unwrap();
    let outcome = device.finish(started.handle.id);
    (outcome, started.result.outcome().unwrap().expect("a finished inspect has its outcome"))
}
fn session(device: &Device, package: &Path) -> (Arc<DeckImportSession>, DeckImportFacts) {
    match inspect(device, package) {
        (JobOutcome::Succeeded, DeckImportInspectOutcome::Inspected { session, facts }) => (session, facts),
        other => panic!("the package inspects, not {other:?}"),
    }
}
fn apply(device: &Device, session: &DeckImportSession, options: DeckImportOptions) -> (JobOutcome, DeckImportApplyOutcome) {
    let started = session.start_apply(options, None, false).unwrap();
    let outcome = device.finish(started.handle.id);
    (outcome, started.result.outcome().unwrap().expect("a finished apply has its outcome"))
}
fn imported(device: &Device, session: &DeckImportSession, options: DeckImportOptions) -> DeckImportResult {
    match apply(device, session, options) {
        (JobOutcome::Succeeded, DeckImportApplyOutcome::Imported { result }) => result,
        other => panic!("the import commits, not {other:?}"),
    }
}
fn undo_name() -> Option<String> {
    match peek_undo(ReviewUndoScope::Global).unwrap() {
        ReviewUndoStatus::Available { step } => Some(step.name),
        ReviewUndoStatus::Unavailable { .. } => None,
    }
}

/// The whole flow: the file's class, the facts, the stored options, the
/// import with its counts and its log in pages, one undo step "Import", a
/// session that commits once, and a second import that updates the edited
/// note and adds no duplicate.
#[test]
fn a_deck_package_is_classified_inspected_applied_paged_undone_and_closed() {
    let root = tempfile::tempdir().unwrap();
    let (package, later, target) = package_and_target(root.path());
    let path = |file: &Path| file.to_str().unwrap().to_owned();

    // P18: the name decides first; whole-collection names go to the other flow.
    assert_eq!(package_file_class_of_name("deck.apkg".into()).unwrap(), Some(PackageFileClass::DeckPackage));
    assert_eq!(package_file_class_of_name("Deck.ZIP".into()).unwrap(), Some(PackageFileClass::DeckPackage));
    for whole in ["all.colpkg", "collection.apkg", "backup-2026-11-23.apkg"] {
        assert_eq!(package_file_class_of_name(whole.into()).unwrap(), Some(PackageFileClass::CollectionPackage));
    }
    assert_eq!(package_file_class_of_name("notes.txt".into()).unwrap(), None);
    assert_eq!(package_file_class(path(&package), "renamed.bin".into()).unwrap(), PackageFileClass::DeckPackage);
    let text = root.path().join("notes.txt");
    std::fs::write(&text, b"plain words").unwrap();
    assert_eq!(package_file_class(path(&text), "notes.txt".into()).unwrap(), PackageFileClass::Unsupported);

    // I5: missing keys read as their defaults.
    let defaults = deck_import_stored_options().unwrap();
    assert_eq!(defaults, DeckImportOptions { merge_notetypes: false, update_notes: DeckImportUpdate::IfNewer,
        update_notetypes: DeckImportUpdate::IfNewer, with_scheduling: false, with_deck_configs: false });

    let (first, facts) = session(&target, &package);
    assert_eq!((facts.format, facts.notes, facts.cards), (DeckPackageFormat::Latest, 2, 2));
    assert_eq!((facts.decks.as_slice(), facts.notetypes.as_slice()), (&["Default".to_owned()][..], &["Basic".to_owned()][..]));
    assert!(!facts.has_scheduling && !facts.has_presets && facts.media_files == 0);
    assert_eq!(target.staging(), 1);
    assert_eq!(first.result().unwrap(), None);
    assert!(first.log_page(DeckImportLogCategory::New, 0, 10).unwrap().is_empty(), "no log before a commit");

    let options = DeckImportOptions { with_scheduling: true, ..defaults };
    let result = imported(&target, &first, options);
    assert_eq!((result.counts.notes_added, result.counts.cards_added, result.counts.notes_updated), (2, 2, 0));
    assert_eq!((result.log.found_notes, result.log.new, result.log.duplicate, result.log.updated), (2, 2, 0, 0));
    assert_eq!(result.undo_step.as_deref(), Some("Import"));
    assert!(result.structure.reasons.is_empty() && result.safety_backup.is_none() && !result.media_pending);
    assert_eq!(first.result().unwrap(), Some(result.clone()));
    // The chosen options are stored by the import (I5).
    assert_eq!(deck_import_stored_options().unwrap(), options);
    // The log in pages, in its order.
    let all = first.log_page(DeckImportLogCategory::New, 0, 10).unwrap();
    assert_eq!(all.iter().map(|entry| entry.fields[0].as_str()).collect::<Vec<_>>(), ["sun", "moon"]);
    assert_eq!(first.log_page(DeckImportLogCategory::New, 0, 1).unwrap(), all[..1]);
    assert_eq!(first.log_page(DeckImportLogCategory::New, 1, 5).unwrap(), all[1..]);
    assert!(first.log_page(DeckImportLogCategory::New, 2, 5).unwrap().is_empty());
    assert!(first.log_page(DeckImportLogCategory::Duplicate, 0, 5).unwrap().is_empty());
    // One undo step, named "Import".
    assert_eq!(undo_name().as_deref(), Some("Import"));
    // A session commits once.
    assert!(matches!(apply(&target, &first, options),
        (JobOutcome::Succeeded, DeckImportApplyOutcome::Refused { refusal: DeckImportRefusal::SessionClosed })));
    first.close().unwrap();
    first.close().unwrap();
    assert_eq!(target.staging(), 0, "a closed session leaves no staging");
    assert!(first.log_page(DeckImportLogCategory::New, 0, 10).unwrap().is_empty());

    // The same package again: both notes are already present.
    let (again, _) = session(&target, &package);
    let same = imported(&target, &again, options);
    assert_eq!((same.counts.notes_added, same.counts.notes_updated, same.log.duplicate, same.log.new), (0, 0, 2, 0));
    assert_eq!(same.undo_step, None, "an import that changed nothing has no undo step");
    drop(again);
    assert_eq!(target.staging(), 0, "a dropped session leaves no staging");

    // The package with the first note edited later: an update, no new note.
    target.clock.now.store(NOW + 200_000, Ordering::SeqCst);
    let (edited, _) = session(&target, &later);
    let updated = imported(&target, &edited, options);
    assert_eq!((updated.counts.notes_added, updated.counts.notes_updated), (0, 1));
    assert_eq!((updated.log.new, updated.log.updated, updated.log.duplicate), (0, 1, 1));
    let entry = &edited.log_page(DeckImportLogCategory::Updated, 0, 5).unwrap()[0];
    assert_eq!(entry.fields[0], "sun-zq2");
    assert_eq!(entry.note_id, all[0].note_id, "the update went to the note the first import added");
    // Undo takes the update back, and only it.
    let undo = start_undo(ReviewUndoScope::Global).unwrap();
    assert_eq!(target.finish(undo.handle.id), JobOutcome::Succeeded);
    assert_eq!(undo_name().as_deref(), Some("Import"), "the first import's step is next");

    // Closing the collection ends the sessions that are still open (I2).
    assert_eq!(target.staging(), 1);
    close_collection().unwrap();
    assert_eq!(target.staging(), 0);
    assert!(edited.result().unwrap().is_none());
}

/// What a file or the device refuses, each as its own case and with
/// nothing left behind.
#[test]
fn refusals_are_typed_and_leave_no_staging() {
    let root = tempfile::tempdir().unwrap();
    let (package, _, target) = package_and_target(root.path());
    let refused = |file: &Path| match inspect(&target, file) {
        (JobOutcome::Succeeded, DeckImportInspectOutcome::Refused { refusal }) => refusal,
        other => panic!("a refusal, not {other:?}"),
    };
    // Neither a package by its name nor by its content.
    let text = root.path().join("notes.txt");
    std::fs::write(&text, b"plain words").unwrap();
    assert_eq!(refused(&text), DeckImportRefusal::UnsupportedFile);
    // A whole-collection name is the other flow's.
    let whole = root.path().join("collection.apkg");
    std::fs::copy(&package, &whole).unwrap();
    assert_eq!(refused(&whole), DeckImportRefusal::CollectionPackage);
    assert_eq!(target.staging(), 0);
    // A deck package by its name whose bytes are no archive.
    let broken = root.path().join("broken.apkg");
    std::fs::write(&broken, b"not an archive at all").unwrap();
    assert!(matches!(refused(&broken), DeckImportRefusal::Corrupt { .. }));
    // A file that is gone.
    assert!(matches!(refused(&root.path().join("gone.apkg")), DeckImportRefusal::Io { .. }));
    // Too little space for the source copy: before anything is written.
    target.space.bytes.store(1024, Ordering::SeqCst);
    let DeckImportRefusal::InsufficientSpace { needed, available } = refused(&package) else { panic!("space") };
    assert!(needed > available && available == 1024);
    assert_eq!(target.staging(), 0);
    // And at the apply's media preflight: the session stays open.
    target.space.bytes.store(u64::MAX, Ordering::SeqCst);
    let (open_session, _) = session(&target, &package);
    let options = deck_import_stored_options().unwrap();
    target.space.bytes.store(1024, Ordering::SeqCst);
    assert!(matches!(apply(&target, &open_session, options), (JobOutcome::Succeeded, DeckImportApplyOutcome::Refused {
        refusal: DeckImportRefusal::InsufficientSpace { .. } })));
    assert_eq!((undo_name(), target.media(), target.backups()), (None, 0, 0));
    target.space.bytes.store(u64::MAX, Ordering::SeqCst);
    assert_eq!(imported(&target, &open_session, options).counts.notes_added, 2);
    open_session.close().unwrap();
    assert_eq!(target.staging(), 0);
    close_collection().unwrap();
}

/// Arms `gate`, starts the job, and cancels it while the work waits there.
fn cancel_at(gate: &Gate, hits: &mpsc::Receiver<()>, go: &mpsc::Sender<()>, start: impl FnOnce() -> JobHandle) -> JobHandle {
    gate.armed.store(true, Ordering::SeqCst);
    let handle = start();
    hits.recv_timeout(Duration::from_secs(20)).expect("the work reaches the pause");
    handle.cancel.cancel().unwrap();
    go.send(()).unwrap();
    handle
}

/// I14.5: a cancel during the inspect removes its staging; a cancel during
/// the apply's plan or its media stage leaves the collection as it was and
/// the session open.
#[test]
fn a_cancel_in_each_phase_writes_nothing() {
    let root = tempfile::tempdir().unwrap();
    let (package, _, target) = package_and_target(root.path());
    let name = || "deck.apkg".to_owned();
    let path = || package.to_str().unwrap().to_owned();

    // The inspect, paused at a space check (before the source copy, or
    // before the extraction) and at a clock read (the analysis).
    for at_space in [true, false] {
        let result = Mutex::new(None);
        let start = || {
            let started = start_deck_import_inspect(path(), name()).unwrap();
            *result.lock().unwrap() = Some(started.result);
            started.handle
        };
        let handle = if at_space { cancel_at(&target.space.gate, &target.space_hits, &target.space_go, start) }
            else { cancel_at(&target.clock.gate, &target.clock_hits, &target.clock_go, start) };
        assert_eq!(target.finish(handle.id), JobOutcome::Cancelled);
        let outcome = result.lock().unwrap().take().unwrap().outcome().unwrap();
        assert!(matches!(outcome, Some(DeckImportInspectOutcome::Refused { refusal: DeckImportRefusal::Cancelled })));
        assert_eq!(target.staging(), 0, "a cancelled inspect removes its staging");
    }

    // The apply: its plan reads the clock, its media stage checks the space.
    let (open_session, _) = session(&target, &package);
    let options = deck_import_stored_options().unwrap();
    for at_space in [false, true] {
        let result = Mutex::new(None);
        let start = || {
            let started = open_session.start_apply(options, None, false).unwrap();
            *result.lock().unwrap() = Some(started.result);
            started.handle
        };
        let handle = if at_space { cancel_at(&target.space.gate, &target.space_hits, &target.space_go, start) }
            else { cancel_at(&target.clock.gate, &target.clock_hits, &target.clock_go, start) };
        assert_eq!(target.finish(handle.id), JobOutcome::Cancelled);
        let outcome = result.lock().unwrap().take().unwrap().outcome().unwrap();
        assert!(matches!(outcome, Some(DeckImportApplyOutcome::Refused { refusal: DeckImportRefusal::Cancelled })));
        assert_eq!((undo_name(), target.media(), target.backups()), (None, 0, 0), "nothing was written");
    }
    // The session is still open.
    assert_eq!(imported(&target, &open_session, options).counts.notes_added, 2);
    close_collection().unwrap();
    assert_eq!(target.staging(), 0);
}

/// The stock Basic note type sorted by its second field: a schema change.
fn sort_basic_by_back(device: &Device) {
    let record = read_notetype(basic()).unwrap();
    let draft = NotetypeStructureDraft {
        notetype_id: record.id,
        base: record.base.clone(),
        name: record.name.clone(),
        css: record.css.clone(),
        fields: record
            .fields
            .iter()
            .map(|f| StructureDraftField {
                origin: Some(f.ord),
                name: f.name.clone(),
                sticky: f.sticky,
                rtl: f.rtl,
                font_name: f.font_name.clone(),
                font_size: f.font_size,
                description: f.description.clone(),
                plain_text: f.plain_text,
                collapsed: f.collapsed,
                exclude_from_search: f.exclude_from_search,
            })
            .collect(),
        templates: record
            .templates
            .iter()
            .map(|t| StructureDraftTemplate {
                origin: Some(t.ord),
                name: t.name.clone(),
                front: t.front.clone(),
                back: t.back.clone(),
                browser_front: t.q_format_browser.clone(),
                browser_back: t.a_format_browser.clone(),
                target_deck_id: t.target_deck_id,
                browser_font_name: t.browser_font_name.clone(),
                browser_font_size: t.browser_font_size,
            })
            .collect(),
        sort_field: Some(1),
        skip_checks: true,
        confirmed: NotetypeConfirmedLosses::default(),
    };
    let saved = start_notetype_structure_save(draft).unwrap();
    assert_eq!(device.finish(saved.handle.id), JobOutcome::Succeeded);
}

/// `sync/meguri-provider` MP7.1's first requirement through the exports: a
/// caller that asks gets a plan's structural reason before anything is
/// written, even with no merge preview, and this call never commits such a
/// plan while it asks (the accepted confirmation is the structure job's).
/// A caller that does not ask (no ready Meguri Sync binding) is asked nothing.
#[test]
fn a_structural_plan_is_confirmed_first_and_never_committed_by_the_asking_apply() {
    let root = tempfile::tempdir().unwrap();
    let source = open(&root.path().join("source"));
    note("g-first", "sun");
    source.clock.now.store(NOW + 90_000, Ordering::SeqCst);
    sort_basic_by_back(&source);
    let package = root.path().join("sorted.apkg");
    test_write_deck_package(package.to_str().unwrap().into()).unwrap();
    close_collection().unwrap();
    drop(source);
    let target = open(&root.path().join("target"));
    target.clock.now.store(NOW + 200_000, Ordering::SeqCst);
    let options = deck_import_stored_options().unwrap();
    let (open_session, _) = session(&target, &package);
    let ask = |confirmation: Option<Arc<DeckImportConfirmation>>, asking: bool| {
        let started = open_session.start_apply(options, confirmation, asking).unwrap();
        assert_eq!(target.finish(started.handle.id), JobOutcome::Succeeded);
        started.result.outcome().unwrap().unwrap()
    };
    let DeckImportApplyOutcome::ConfirmationRequired { request } = ask(None, true) else { panic!("asks first") };
    assert_eq!(request.reasons, [DeckImportStructuralReason::SortFieldChanged]);
    assert!(request.preview.emptied_fields.is_empty() && request.preview.removed_notetypes.is_empty()
        && request.preview.dropped_values.is_empty());
    assert_eq!((undo_name(), target.backups()), (None, 0), "nothing was written or backed up");
    // The asking apply does not commit the confirmed plan either: the job does.
    let DeckImportApplyOutcome::ConfirmationRequired { request: again } = ask(Some(request.confirmation.clone()), true) else {
        panic!("the asking apply never commits a structural plan")
    };
    assert_eq!(again.reasons, [DeckImportStructuralReason::SortFieldChanged]);
    assert_eq!((undo_name(), target.backups()), (None, 0));
    // Without the question (iCloud, Off, no ready binding): it commits, with
    // the safety backup a structural edit takes.
    let DeckImportApplyOutcome::Imported { result } = ask(None, false) else { panic!("commits alone") };
    assert_eq!(result.structure.reasons, [DeckImportStructuralReason::SortFieldChanged]);
    assert!(result.safety_backup.is_some() && target.backups() == 1);
    assert_eq!(result.counts.notes_added, 1);
    close_collection().unwrap();
}
