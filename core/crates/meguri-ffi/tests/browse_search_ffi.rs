//! U264's Browse search session through its real exports on an opened
//! collection (`browser/result-sets` H2 to H12, H15): events of each
//! request in order, pages with deferred Question and Answer cells and the
//! cells themselves, reverse and its stale revision, resort, the toggle,
//! selection resolution and mapping, invalidation by the change sets of a
//! tag edit and a note deletion, search errors with UTF-16 ranges, the
//! argument refusals, a cancelled request, and closing.
use meguri_ffi::{
    CancelToken, CoreError, CoreEvent, CoreEventKind, CoreEventListener, JobOutcome,
    browse_search::*,
    collection::{self, CollectionLocation, DiskCapacity, DiskSpaceProvider, HostClock},
    excerpts::ExcerptQuestion,
    notes::{AddNoteRequest, add_note, start_delete_notes},
    notetypes::{AddNotetypeRequest, NewNotetypeTemplate, add_notetype},
    tags::add_note_tags,
};
use std::{
    sync::{Arc, mpsc},
    time::Duration,
};

struct Clock;
impl HostClock for Clock {
    fn now_ms(&self) -> i64 {
        1_894_701_600_000
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

fn open(dir: &tempfile::TempDir) -> mpsc::Receiver<CoreEvent> {
    let collection = dir.path().join("collection");
    let backups = dir.path().join("backups");
    std::fs::create_dir_all(&collection).unwrap();
    std::fs::create_dir_all(&backups).unwrap();
    meguri_engine::bootstrap::create(
        &collection.join("collection.anki2"),
        &meguri_model::clock::FixedClock::new(1_894_701_600_000, meguri_model::clock::UtcOffset::UTC),
        &meguri_model::rng::SeededRandom::new(7),
    )
    .unwrap();
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
    rx
}

fn template(name: &str, front: &str) -> NewNotetypeTemplate {
    NewNotetypeTemplate {
        name: name.into(),
        front: front.into(),
        back: "{{FrontSide}}".into(),
        browser_front: String::new(),
        browser_back: String::new(),
    }
}

/// Every event of request `seq`, up to its terminal one; no event of
/// another request comes in between.
fn request(rx: &mpsc::Receiver<BrowseSearchEvent>, seq: u64) -> Vec<BrowseSearchEvent> {
    let mut events = Vec::new();
    loop {
        let event = rx.recv_timeout(Duration::from_secs(20)).unwrap();
        let (got, terminal) = match &event {
            BrowseSearchEvent::FirstPage { seq, .. } | BrowseSearchEvent::Progress { seq, .. } => (*seq, false),
            BrowseSearchEvent::Complete { seq, .. }
            | BrowseSearchEvent::Failed { seq, .. }
            | BrowseSearchEvent::Cancelled { seq } => (*seq, true),
        };
        assert_eq!(got, seq, "{event:?}");
        events.push(event);
        if terminal {
            return events;
        }
    }
}

/// A request that completes: its handle, first page and count.
fn completed(rx: &mpsc::Receiver<BrowseSearchEvent>, seq: u64) -> (u64, Vec<i64>, u64) {
    match request(rx, seq).as_slice() {
        [
            BrowseSearchEvent::FirstPage { handle, ids, .. },
            BrowseSearchEvent::Complete {
                handle: done,
                count,
                revision: 0,
                ..
            },
        ] if done == handle => (*handle, ids.clone(), *count),
        other => panic!("{other:?}"),
    }
}

fn failed(rx: &mpsc::Receiver<BrowseSearchEvent>, seq: u64) -> BrowseRequestError {
    match request(rx, seq).as_slice() {
        [BrowseSearchEvent::Failed { error, .. }] => error.clone(),
        other => panic!("{other:?}"),
    }
}

fn sort(key: &str, direction: BrowseSortDirection) -> BrowseSort {
    BrowseSort {
        key: key.into(),
        direction,
    }
}

fn range(start: u64, end: u64) -> BrowseIndexRange {
    BrowseIndexRange { start, end }
}

fn sort_field(row: &BrowseRowResult) -> &str {
    match row {
        BrowseRowResult::Row { row } => match &row.cells[0] {
            BrowseCell::Text { text } => text,
            other => panic!("{other:?}"),
        },
        other => panic!("{other:?}"),
    }
}

fn browse(failure: BrowseFailure) -> CoreError {
    CoreError::Browse { failure }
}

#[test]
fn a_search_session_pages_sorts_toggles_and_maps_through_its_exports() {
    let dir = tempfile::tempdir().unwrap();
    let core = open(&dir);
    let notetype = add_notetype(AddNotetypeRequest {
        name: "Duo".into(),
        cloze: false,
        fields: vec!["Front".into(), "Back".into()],
        templates: vec![template("Forward", "{{Front}}"), template("Reverse", "{{Back}}")],
        css: Some(String::new()),
    })
    .unwrap()
    .id;
    // Three notes of two cards each, in the order their sort field sorts.
    let notes: Vec<(i64, Vec<i64>)> = [["alpha", "one"], ["beta", "two"], ["gamma", "three"]]
        .iter()
        .map(|fields| {
            let added = add_note(AddNoteRequest {
                notetype_id: notetype,
                fields: fields.iter().map(|f| (*f).to_owned()).collect(),
                tags: Vec::new(),
                deck_id: 1,
                guid: None,
                update_add_defaults: false,
            })
            .unwrap();
            let mut cards = added.cards;
            cards.sort_unstable();
            (added.id, cards)
        })
        .collect();
    let cards: Vec<i64> = notes.iter().flat_map(|(_, cards)| cards.clone()).collect();

    let (tx, rx) = mpsc::channel();
    let session = open_search_session(Arc::new(Searches(tx))).unwrap();

    // H3, H4: a whole-collection search in cards mode, by sort field.
    let seq = session
        .start_search(String::new(), BrowseMode::Cards, sort("noteFld", BrowseSortDirection::Ascending), None)
        .unwrap();
    assert_eq!(seq, 1);
    let (first, page, count) = completed(&rx, seq);
    assert_eq!((page.clone(), count), (cards.clone(), 6));
    assert_eq!(
        session.handle_state(first).unwrap(),
        BrowseHandleState {
            mode: BrowseMode::Cards,
            revision: 0,
            count: 6,
            complete: true,
            sort: sort("noteFld", BrowseSortDirection::Ascending),
        }
    );

    // H7: a page defers Question and Answer; `qa_cells` gives them.
    let rows = session
        .rows(first, 0, range(0, 6), vec!["noteFld".into(), "question".into(), "answer".into()])
        .unwrap();
    assert_eq!(rows.len(), 6);
    assert_eq!(sort_field(&rows[0]), "alpha");
    assert_eq!(sort_field(&rows[5]), "gamma");
    let BrowseRowResult::Row { row } = &rows[2] else {
        panic!("{:?}", rows[2])
    };
    assert_eq!((row.id, row.note_id), (cards[2], notes[1].0));
    assert_eq!(&row.cells[1..], &[BrowseCell::Deferred, BrowseCell::Deferred]);
    let cells = session.qa_cells(first, 0, range(0, 2), BrowseQaWhich::Question, None).unwrap();
    assert_eq!(
        cells,
        vec![
            BrowseQaResult::Cells {
                question: Some(ExcerptQuestion::Text { text: "alpha".into() }),
                answer: None,
            },
            BrowseQaResult::Cells {
                question: Some(ExcerptQuestion::Text { text: "one".into() }),
                answer: None,
            },
        ]
    );

    // H8: reverse flips the list in memory; the old revision is stale.
    assert_eq!(session.reverse(first, 0).unwrap(), 1);
    assert_eq!(
        sort_field(&session.rows(first, 1, range(0, 1), vec!["noteFld".into()]).unwrap()[0]),
        "gamma"
    );
    assert_eq!(
        session.rows(first, 0, range(0, 1), vec!["noteFld".into()]),
        Err(browse(BrowseFailure::StaleRevision { current: 1 }))
    );
    assert_eq!(
        session.handle_state(first).unwrap().sort,
        sort("noteFld", BrowseSortDirection::Descending)
    );

    // H8: a resort is a request of its own, without a predicate run.
    let seq = session
        .resort(first, sort("noteFld", BrowseSortDirection::Ascending), None)
        .unwrap();
    assert_eq!(seq, 2);
    let (cards_handle, page, count) = completed(&rx, seq);
    assert_eq!((page, count), (cards.clone(), 6));

    // H11: the toggle gives the notes of the same search.
    let seq = session
        .toggle(cards_handle, sort("noteFld", BrowseSortDirection::Ascending), None)
        .unwrap();
    let (notes_handle, page, count) = completed(&rx, seq);
    let note_ids: Vec<i64> = notes.iter().map(|(id, _)| *id).collect();
    assert_eq!((page, count), (note_ids.clone(), 3));
    // H6: the two newest complete handles stay; the first is released.
    assert_eq!(session.handle_state(first), Err(browse(BrowseFailure::UnknownHandle)));

    // H12: resolution, index lookup and mapping.
    assert_eq!(
        session
            .resolve_selection(cards_handle, 0, vec![range(0, 3)], BrowseMode::Notes)
            .unwrap(),
        vec![notes[0].0, notes[1].0]
    );
    assert_eq!(
        session
            .resolve_selection(notes_handle, 0, vec![range(2, 3), range(0, 1)], BrowseMode::Cards)
            .unwrap(),
        [notes[0].1.clone(), notes[2].1.clone()].concat()
    );
    let selection = session.selection(notes_handle, 0, vec![range(0, 3)]).unwrap();
    assert_eq!(selection.resolve(BrowseMode::Notes).unwrap(), note_ids);
    assert_eq!(
        session.index_of(notes_handle, 0, BrowseMode::Cards, notes[1].1[1]).unwrap(),
        Some(1)
    );
    assert_eq!(
        session.index_of(cards_handle, 0, BrowseMode::Notes, notes[2].0).unwrap(),
        Some(4)
    );
    assert_eq!(session.index_of(cards_handle, 0, BrowseMode::Cards, -1).unwrap(), None);
    assert_eq!(
        session
            .map_selection(
                BrowseMapFrom::Handle {
                    handle: cards_handle,
                    revision: 0,
                },
                BrowseSelected::Ranges {
                    ranges: vec![range(0, 3)],
                },
                Some(cards[2]),
                notes_handle,
                0,
            )
            .unwrap(),
        BrowseMappedSelection {
            selected: vec![range(0, 2)],
            current: Some(1),
        }
    );
    assert_eq!(
        session
            .map_selection(
                BrowseMapFrom::Mode {
                    mode: BrowseMode::Notes,
                },
                BrowseSelected::Ids {
                    ids: vec![notes[1].0],
                },
                Some(notes[1].0),
                cards_handle,
                0,
            )
            .unwrap(),
        BrowseMappedSelection {
            selected: vec![range(2, 4)],
            current: Some(2),
        }
    );

    // H10: a tag edit refetches the note's cards; a deletion removes rows
    // and changes the revision. Change sets cross back as Swift got them.
    let tagged = add_note_tags(vec![notes[0].0], "picked".into()).unwrap().summary;
    let tagged = SequencedChangeSet {
        seq: tagged.seq,
        changes: tagged.changes,
    };
    assert_eq!(
        session.invalidated_rows(cards_handle, 0, vec![tagged.clone()]).unwrap(),
        BrowseInvalidation {
            removed: Vec::new(),
            refetch_all: false,
            refetch: vec![0, 1],
            revision: 0,
            complete: true,
            count: 6,
        }
    );
    let deletion = start_delete_notes(vec![notes[2].0]).unwrap();
    assert_eq!(finish(&core, deletion.handle.id), JobOutcome::Succeeded);
    let deleted = deletion.result.result().unwrap().unwrap().summary.unwrap();
    let deleted = SequencedChangeSet {
        seq: deleted.seq,
        changes: deleted.changes,
    };
    // A change set already applied is skipped.
    let invalidation = session
        .invalidated_rows(cards_handle, 0, vec![tagged, deleted.clone()])
        .unwrap();
    assert_eq!(
        invalidation,
        BrowseInvalidation {
            removed: vec![4, 5],
            refetch_all: false,
            refetch: Vec::new(),
            revision: 1,
            complete: true,
            count: 4,
        }
    );
    let notes_invalidation = session.invalidated_rows(notes_handle, 0, vec![deleted]).unwrap();
    assert_eq!((notes_invalidation.removed, notes_invalidation.count), (vec![2], 2));
    // H7: rows read the collection now; an index past the list is refused.
    assert_eq!(
        session.rows(cards_handle, 1, range(0, 5), vec!["noteFld".into()]),
        Err(browse(BrowseFailure::OutOfRange))
    );

    // H7's argument refusals.
    assert!(matches!(
        session.rows(cards_handle, 1, range(0, 1), vec!["bogus".into()]),
        Err(CoreError::InvalidArgument { .. })
    ));
    assert!(matches!(
        session.rows(cards_handle, 1, range(0, 1001), vec!["noteFld".into()]),
        Err(CoreError::InvalidArgument { .. })
    ));

    // H4, H15: a search error's range is in UTF-16 units of the text; a
    // sort the mode cannot take is `unsortable_column`.
    let text = "\u{1D11E} is:bogus";
    let seq = session
        .start_search(text.into(), BrowseMode::Cards, sort("noteFld", BrowseSortDirection::Ascending), None)
        .unwrap();
    let BrowseRequestError::Search { error } = failed(&rx, seq) else {
        panic!("not a search error")
    };
    assert_eq!(error.kind, "invalid_state");
    assert_eq!(error.provided.as_deref(), Some("bogus"));
    assert_eq!((error.start, error.end), (3, 11));
    assert!(!error.message.is_empty());
    let seq = session
        .start_search(String::new(), BrowseMode::Cards, sort("question", BrowseSortDirection::Ascending), None)
        .unwrap();
    assert_eq!(
        failed(&rx, seq),
        BrowseRequestError::Core {
            error: browse(BrowseFailure::UnsortableColumn)
        }
    );

    // H3: the caller's token cancels its request.
    let cancel = CancelToken::new_arc();
    cancel.trigger();
    let seq = session
        .start_search(
            String::new(),
            BrowseMode::Notes,
            sort("noteFld", BrowseSortDirection::Ascending),
            Some(Arc::clone(&cancel)),
        )
        .unwrap();
    assert_eq!(request(&rx, seq), vec![BrowseSearchEvent::Cancelled { seq }]);

    // H2: closing releases every handle; later calls refuse.
    session.close().unwrap();
    assert_eq!(session.handle_state(cards_handle), Err(browse(BrowseFailure::UnknownHandle)));
    assert_eq!(
        selection.resolve(BrowseMode::Notes),
        Err(browse(BrowseFailure::UnknownHandle))
    );
    assert_eq!(
        session.start_search(String::new(), BrowseMode::Cards, sort("noteFld", BrowseSortDirection::Ascending), None),
        Err(browse(BrowseFailure::UnknownHandle))
    );
    session.close().unwrap();
}
