//! Find and Replace through its real exports on an opened collection
//! (`browser/find-replace` F9 to F11, `browser/workspace` V9): the preview
//! on Selected, All Found and All Notes equals the run it confirms; F11's
//! refusals arrive typed, `CountChanged` with its count, `InvalidRegex`
//! with its message only; a newer preview cancels the older one; a cancel
//! before the commit writes nothing; the run is admitted by the review
//! service, so an answer the session accepted is written first.
use meguri_ffi::{
    CoreError, CoreEvent, CoreEventKind, CoreEventListener, JobOutcome,
    browse_search::*,
    bulk_actions::{BulkActionJobStart, BulkActionResult},
    collection::{self, CollectionLocation, DiskCapacity, DiskSpaceProvider, HostClock},
    find_replace::*,
    notes::{AddNoteRequest, add_note, read_note},
    notetypes::{AddNotetypeRequest, NewNotetypeTemplate, add_notetype},
    review_session::ReviewUndoScope,
    undo::{peek_redo, peek_undo},
};
use std::{
    sync::{Arc, mpsc},
    time::Duration,
};

const NOW: i64 = 1_894_701_600_000;

struct Clock;
impl HostClock for Clock {
    fn now_ms(&self) -> i64 {
        NOW
    }
    fn offset_from_utc(&self, _: i64) -> i32 {
        0
    }
    fn zone_id(&self) -> String {
        "UTC".into()
    }
}
struct Space;
impl DiskSpaceProvider for Space {
    fn available_bytes(&self, _: String) -> DiskCapacity {
        DiskCapacity::Available { bytes: u64::MAX }
    }
}
struct Listener(mpsc::Sender<CoreEvent>);
impl CoreEventListener for Listener {
    fn on_event(&self, event: CoreEvent) {
        let _ = self.0.send(event);
    }
}
struct Searches(mpsc::Sender<BrowseSearchEvent>);
impl BrowseSearchListener for Searches {
    fn on_event(&self, event: BrowseSearchEvent) {
        let _ = self.0.send(event);
    }
}

fn finish(rx: &mpsc::Receiver<CoreEvent>, id: u64) -> JobOutcome {
    loop {
        let event = rx.recv_timeout(Duration::from_secs(20)).unwrap();
        if let CoreEventKind::JobFinished { id: got, outcome } = event.kind
            && got == id
        {
            return outcome;
        }
    }
}

/// The outcomes of the jobs `ids`, in that order, whichever order they end
/// in (`finish` drops the events of other jobs).
#[cfg(feature = "test-hooks")]
fn finish_all(rx: &mpsc::Receiver<CoreEvent>, ids: &[u64]) -> Vec<JobOutcome> {
    let mut outcomes: Vec<Option<JobOutcome>> = vec![None; ids.len()];
    while outcomes.iter().any(Option::is_none) {
        let event = rx.recv_timeout(Duration::from_secs(20)).unwrap();
        if let CoreEventKind::JobFinished { id, outcome } = event.kind
            && let Some(at) = ids.iter().position(|wanted| *wanted == id)
        {
            outcomes[at] = Some(outcome);
        }
    }
    outcomes.into_iter().map(Option::unwrap).collect()
}

/// Opens a new collection with FSRS on (answers need it, `sched/answering`
/// A9) and adds the note type Quorv (Front, Back; one card per note).
fn open(dir: &tempfile::TempDir) -> (mpsc::Receiver<CoreEvent>, i64) {
    let collection = dir.path().join("collection");
    let backups = dir.path().join("backups");
    std::fs::create_dir_all(&collection).unwrap();
    std::fs::create_dir_all(&backups).unwrap();
    let path = collection.join("collection.anki2");
    meguri_engine::bootstrap::create(
        &path,
        &meguri_model::clock::FixedClock::new(NOW, meguri_model::clock::UtcOffset::UTC),
        &meguri_model::rng::SeededRandom::new(7),
    )
    .unwrap();
    let db = meguri_storage::raw::PlainDatabase::open(&path).unwrap();
    for (key, value) in [("fsrs", "true"), ("loadBalancerEnabled", "false")] {
        db.execute(
            "INSERT OR REPLACE INTO config (KEY, usn, mtime_secs, val) VALUES (?, 0, 0, ?)",
            &[
                meguri_storage::raw::RawValue::text(key),
                meguri_storage::raw::RawValue::Blob(value.as_bytes().to_vec()),
            ],
        )
        .unwrap();
    }
    db.close().unwrap();
    let (tx, rx) = mpsc::channel();
    meguri_ffi::events::set_event_listener(Arc::new(Listener(tx))).unwrap();
    let handle = collection::start_open(
        CollectionLocation {
            manual_transfer_directory: dir.path().join("ManualTransfers").to_string_lossy().into_owned(),
            collection_directory: collection.to_str().unwrap().into(),
            backups_directory: backups.to_str().unwrap().into(),
            app_group_snapshot: dir.path().join("widget.snapshot").to_str().unwrap().into(),
        },
        Arc::new(Clock),
        Arc::new(Space),
        7,
    )
    .unwrap();
    assert_eq!(finish(&rx, handle.id), JobOutcome::Succeeded);
    let notetype = add_notetype(AddNotetypeRequest {
        name: "Quorv".into(),
        cloze: false,
        fields: vec!["Front".into(), "Back".into()],
        templates: vec![NewNotetypeTemplate {
            name: "Forward".into(),
            front: "{{Front}}".into(),
            back: "{{FrontSide}}".into(),
            browser_front: String::new(),
            browser_back: String::new(),
        }],
        css: Some(String::new()),
    })
    .unwrap()
    .id;
    (rx, notetype)
}

fn note(notetype: i64, front: &str, tags: &[&str]) -> i64 {
    add_note(AddNoteRequest {
        notetype_id: notetype,
        fields: vec![front.into(), "back".into()],
        tags: tags.iter().map(|tag| (*tag).to_owned()).collect(),
        deck_id: 1,
        guid: None,
        update_add_defaults: false,
    })
    .unwrap()
    .id
}

fn request(find: &str, replace: &str, within: FindReplaceIn) -> FindReplaceRequest {
    FindReplaceRequest {
        find: find.into(),
        replace: replace.into(),
        regex: false,
        ignore_case: true,
        whole_words: false,
        within,
    }
}

fn front() -> FindReplaceIn {
    FindReplaceIn::Field { name: "Front".into() }
}

/// Every note's fields and tags, by id.
fn notes(ids: &[i64]) -> Vec<(Vec<String>, Vec<String>)> {
    ids.iter()
        .map(|id| {
            let note = read_note(*id).unwrap();
            (note.fields, note.tags)
        })
        .collect()
}

fn preview(
    rx: &mpsc::Receiver<CoreEvent>,
    previews: &FindReplacePreviews,
    target: Option<Arc<BrowseSelection>>,
    request: FindReplaceRequest,
) -> Result<FindReplacePreview, CoreError> {
    let start = previews.start(target, request).unwrap();
    match finish(rx, start.handle.id) {
        JobOutcome::Succeeded => Ok(start.result.result().unwrap().expect("the preview is set before JobFinished")),
        JobOutcome::Failed { error } => {
            assert_eq!(start.result.result(), Err(error.clone()), "the result holds the job's error");
            Err(error)
        }
        JobOutcome::Cancelled => {
            assert_eq!(start.result.result(), Err(CoreError::Cancelled));
            Err(CoreError::Cancelled)
        }
    }
}

fn run(rx: &mpsc::Receiver<CoreEvent>, start: BulkActionJobStart) -> Result<BulkActionResult, CoreError> {
    match finish(rx, start.handle.id) {
        JobOutcome::Succeeded => Ok(start.result.result().unwrap().expect("the result is set before JobFinished")),
        JobOutcome::Failed { error } => {
            assert_eq!(start.result.result(), Err(error.clone()), "the result holds the job's error");
            Err(error)
        }
        JobOutcome::Cancelled => {
            assert_eq!(start.result.result(), Err(CoreError::Cancelled));
            Err(CoreError::Cancelled)
        }
    }
}

fn refused(refusal: FindReplaceRefusal) -> CoreError {
    CoreError::FindReplaceRefused { refusal }
}

/// A Notes-mode search of every note, sorted by the sort field.
fn search(session: &BrowseSearchSession, events: &mpsc::Receiver<BrowseSearchEvent>) -> u64 {
    let seq = session
        .start_search(
            String::new(),
            BrowseMode::Notes,
            BrowseSort {
                key: "noteFld".into(),
                direction: BrowseSortDirection::Ascending,
            },
            None,
        )
        .unwrap();
    loop {
        match events.recv_timeout(Duration::from_secs(20)).unwrap() {
            BrowseSearchEvent::Complete { seq: done, handle, .. } if done == seq => return handle,
            BrowseSearchEvent::Failed { error, .. } => panic!("{error:?}"),
            _ => {}
        }
    }
}

fn range(start: u64, end: u64) -> Vec<BrowseIndexRange> {
    vec![BrowseIndexRange { start, end }]
}

#[test]
fn the_preview_equals_the_run_it_confirms_and_refusals_arrive_typed() {
    let dir = tempfile::tempdir().unwrap();
    let (rx, notetype) = open(&dir);
    // Sorted by Front: amber, apple, birch, plum.
    let amber = note(notetype, "amber pip", &["xylo"]);
    let apple = note(notetype, "apple pip", &["frun"]);
    let birch = note(notetype, "birch", &["xylo"]);
    let plum = note(notetype, "plum pip", &[]);
    let all = [amber, apple, birch, plum];
    let (tx, events) = mpsc::channel();
    let session = open_search_session(Arc::new(Searches(tx))).unwrap();
    let handle = search(&session, &events);
    let found = session.selection(handle, 0, range(0, 4)).unwrap();
    let selected = session.selection(handle, 0, range(1, 3)).unwrap();
    let previews = FindReplacePreviews::new().unwrap();

    // Selected (apple, birch): one note changes of two.
    let shown = preview(&rx, &previews, Some(Arc::clone(&selected)), request("pip", "seed", front())).unwrap();
    assert_eq!((shown.changed, shown.considered), (1, 2));
    assert_eq!(
        shown.entries,
        vec![FindReplacePreviewEntry {
            note_id: apple,
            sort_field: "apple pip".into(),
            field: Some("Front".into()),
            spans: vec![
                FindReplaceSpan {
                    edit: FindReplaceEdit::Kept,
                    text: "apple ".into(),
                },
                FindReplaceSpan {
                    edit: FindReplaceEdit::Removed,
                    text: "pip".into(),
                },
                FindReplaceSpan {
                    edit: FindReplaceEdit::Inserted,
                    text: "seed".into(),
                },
            ],
        }]
    );

    // All Found: three notes change, in result order; the run confirms the
    // preview's count and writes what it showed.
    let all_found = preview(&rx, &previews, Some(Arc::clone(&found)), request("pip", "seed", front())).unwrap();
    assert_eq!((all_found.changed, all_found.considered), (3, 4));
    let listed: Vec<i64> = all_found.entries.iter().map(|entry| entry.note_id).collect();
    assert_eq!(listed, [amber, apple, plum]);
    let done = run(
        &rx,
        start_find_replace(Some(Arc::clone(&found)), request("pip", "seed", front()), all_found.changed).unwrap(),
    )
    .unwrap();
    assert_eq!((done.affected, done.considered), (all_found.changed, all_found.considered));
    assert_eq!(done.summary.undo_name.as_deref(), Some("Find and Replace"));
    let fronts: Vec<String> = notes(&all).into_iter().map(|(fields, _)| fields[0].clone()).collect();
    assert_eq!(fronts, ["amber seed", "apple seed", "birch", "plum seed"]);

    // All Notes in Tags: the "Update Tag" step.
    let tags = preview(&rx, &previews, None, request("xylo", "tumb", FindReplaceIn::Tags)).unwrap();
    assert_eq!((tags.changed, tags.considered), (2, 4));
    assert_eq!(tags.entries[0].field, None);
    let done = run(
        &rx,
        start_find_replace(None, request("xylo", "tumb", FindReplaceIn::Tags), tags.changed).unwrap(),
    )
    .unwrap();
    assert_eq!((done.affected, done.considered), (2, 4));
    assert_eq!(done.summary.undo_name.as_deref(), Some("Update Tag"));

    // CountChanged: a note added after the preview makes the run write one
    // note more than was confirmed. It refuses with the count it read,
    // writes nothing and keeps both histories.
    let counted = preview(&rx, &previews, None, request("seed", "kern", front())).unwrap();
    assert_eq!(counted.changed, 3);
    let late = note(notetype, "late seed", &[]);
    let every = [amber, apple, birch, plum, late];
    let before = notes(&every);
    let undo = peek_undo(ReviewUndoScope::Global).unwrap();
    let redo = peek_redo(ReviewUndoScope::Global).unwrap();
    assert_eq!(
        run(
            &rx,
            start_find_replace(None, request("seed", "kern", front()), counted.changed).unwrap()
        ),
        Err(refused(FindReplaceRefusal::CountChanged { notes: 4 }))
    );
    assert_eq!(notes(&every), before);
    assert_eq!(peek_undo(ReviewUndoScope::Global).unwrap(), undo);
    assert_eq!(peek_redo(ReviewUndoScope::Global).unwrap(), redo);
    // The count the refusal carried is the one the run accepts.
    let done = run(
        &rx,
        start_find_replace(None, request("seed", "kern", front()), 4).unwrap(),
    )
    .unwrap();
    assert_eq!(done.affected, 4);

    // InvalidRegex: the message only, in the preview and in the run.
    let mut bad = request("(un", "x", FindReplaceIn::AllFields);
    bad.regex = true;
    let Err(CoreError::FindReplaceRefused {
        refusal: FindReplaceRefusal::InvalidRegex { message },
    }) = preview(&rx, &previews, None, bad.clone())
    else {
        panic!("an invalid pattern is F10's error")
    };
    assert!(!message.is_empty());
    assert_eq!(
        run(&rx, start_find_replace(None, bad, 0).unwrap()),
        Err(refused(FindReplaceRefusal::InvalidRegex { message }))
    );
    // F7 step 1, in the preview and in the run.
    let spaced = request("tumb", "two words", FindReplaceIn::Tags);
    assert_eq!(
        preview(&rx, &previews, None, spaced.clone()),
        Err(refused(FindReplaceRefusal::SeparatorInReplacement))
    );
    assert_eq!(
        run(&rx, start_find_replace(None, spaced, 0).unwrap()),
        Err(refused(FindReplaceRefusal::SeparatorInReplacement))
    );

    // The framework's refusals: a selection of another revision, an empty
    // field name.
    let stale = session.selection(handle, 7, range(0, 1)).unwrap();
    assert!(matches!(
        preview(&rx, &previews, Some(Arc::clone(&stale)), request("a", "b", front())),
        Err(CoreError::InvalidArgument { .. })
    ));
    assert!(matches!(
        run(&rx, start_find_replace(Some(stale), request("a", "b", front()), 1).unwrap()),
        Err(CoreError::InvalidArgument { .. })
    ));
    let unnamed = request("a", "b", FindReplaceIn::Field { name: String::new() });
    assert!(matches!(
        run(&rx, start_find_replace(None, unnamed, 1).unwrap()),
        Err(CoreError::InvalidArgument { .. })
    ));
    assert_eq!(notes(&every).len(), 5);
    session.close().unwrap();
}

/// F11 and K2: the run is admitted by the review service. An answer the
/// session accepted just before is written first, the run then rewrites the
/// answered card's note, and once the run ended the session takes answers
/// again.
#[test]
fn the_run_writes_an_accepted_answer_first_and_releases_the_session() {
    use meguri_ffi::card_info::{CardInfoResult, card_info};
    use meguri_ffi::review_session::{
        ReviewAnswerOutcome, ReviewRating, ReviewStartResult, answer_session, session_head, start_session,
    };

    let dir = tempfile::tempdir().unwrap();
    let (rx, notetype) = open(&dir);
    for front in ["kelv one", "kelv two", "kelv three"] {
        note(notetype, front, &[]);
    }
    let start = start_session(1, None).unwrap();
    assert_eq!(finish(&rx, start.handle.id), JobOutcome::Succeeded);
    let Some(ReviewStartResult::Started { session_id, head, .. }) = start.result.result().unwrap() else {
        panic!("the review starts")
    };
    let answered = head.items[0].clone();
    let outcome = answer_session(
        session_id,
        answered.card_id,
        ReviewRating::Good,
        answered.state_token.clone(),
        NOW,
        1_000,
    )
    .unwrap();
    assert!(matches!(outcome, ReviewAnswerOutcome::Accepted { .. }), "{outcome:?}");
    // At once, before the answer is known to be written: the run on the
    // answered card's note.
    let done = run(
        &rx,
        start_find_replace(None, request("kelv", "wrack", front()), 3).unwrap(),
    )
    .unwrap();
    assert_eq!(done.affected, 3);
    let CardInfoResult::Found { snapshot } = card_info(answered.card_id).unwrap() else {
        panic!("the answered card is there")
    };
    assert_eq!((snapshot.raw.reviews, snapshot.history.len()), (1, 1), "the answer was written");
    assert!(read_note(answered.note_id).unwrap().fields[0].starts_with("wrack"));
    // The admission ended with the run: the next card is answered.
    let head = session_head(session_id, 1).unwrap();
    let next = &head.items[0];
    assert!(matches!(
        answer_session(session_id, next.card_id, ReviewRating::Good, next.state_token.clone(), NOW, 1_000).unwrap(),
        ReviewAnswerOutcome::Accepted { .. }
    ));
}

/// F10, V9: a newer preview of the same sheet cancels the older one, in
/// call order; a cancel before the commit writes nothing and records no
/// step (F9, R1860). The maintenance reader, which both read, is held
/// meanwhile, so neither can end before the cancel.
#[cfg(feature = "test-hooks")]
#[test]
fn a_newer_preview_cancels_the_older_and_a_cancelled_run_writes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let (rx, notetype) = open(&dir);
    let ids = [note(notetype, "fen one", &[]), note(notetype, "fen two", &[])];
    let before = notes(&ids);
    let collection = meguri_ffi::test_hooks::collection_for_testing().unwrap();
    let previews = FindReplacePreviews::new().unwrap();

    let held = collection.reader(meguri_storage::ReaderRole::Maintenance).unwrap();
    let older = previews.start(None, request("fen", "bog", front())).unwrap();
    let newer = previews.start(None, request("fen", "moss", front())).unwrap();
    assert!(older.handle.cancel.is_cancelled().unwrap(), "the newer preview cancels the older");
    assert!(!newer.handle.cancel.is_cancelled().unwrap());
    let cancelled = start_find_replace(None, request("fen", "moss", front()), 2).unwrap();
    cancelled.handle.cancel.cancel().unwrap();
    let undo = peek_undo(ReviewUndoScope::Global).unwrap();
    drop(held);

    assert_eq!(
        finish_all(&rx, &[older.handle.id, newer.handle.id, cancelled.handle.id]),
        [JobOutcome::Cancelled, JobOutcome::Succeeded, JobOutcome::Cancelled]
    );
    assert_eq!(older.result.result(), Err(CoreError::Cancelled));
    let shown = newer.result.result().unwrap().unwrap();
    assert_eq!((shown.changed, shown.considered), (2, 2));
    assert_eq!(cancelled.result.result(), Err(CoreError::Cancelled));
    assert_eq!(notes(&ids), before);
    assert_eq!(peek_undo(ReviewUndoScope::Global).unwrap(), undo);

    // Dropping the sheet's previews cancels its newest preview.
    let held = collection.reader(meguri_storage::ReaderRole::Maintenance).unwrap();
    let last = previews.start(None, request("fen", "peat", front())).unwrap();
    drop(previews);
    assert!(last.handle.cancel.is_cancelled().unwrap());
    drop(held);
    assert_eq!(finish(&rx, last.handle.id), JobOutcome::Cancelled);
}
