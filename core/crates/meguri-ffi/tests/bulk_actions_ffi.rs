//! U87's bulk actions through their real exports on an opened collection:
//! Change Deck to a typed name, the scheduling actions admitted by the
//! review service, a refused empty list as the job's failure, the tag
//! actions and Delete Notes with its second count; the same actions on a
//! Browse selection, which the job resolves in the core (K1).
use meguri_ffi::{
    CoreError, CoreEvent, CoreEventKind, CoreEventListener, JobOutcome,
    browse_search::{
        BrowseIndexRange, BrowseMode, BrowseSearchEvent, BrowseSearchListener, BrowseSort, BrowseSortDirection,
        open_search_session,
    },
    bulk_actions::{
        BulkActionResult, BulkCardAction, BulkNoteAction, start_bulk_card_action, start_bulk_card_action_on_selection,
        start_bulk_note_action, start_bulk_note_action_on_selection,
    },
    collection::{self, CollectionLocation, DiskCapacity, DiskSpaceProvider, HostClock},
    decks::add_deck,
    notes::{AddNoteRequest, add_note, read_note},
    notetypes::{AddNotetypeRequest, NewNotetypeTemplate, add_notetype},
    review_session::ReviewUndoScope,
    undo::start_undo,
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

fn note(notetype: i64, front: &str) -> (i64, i64) {
    let added = add_note(AddNoteRequest {
        notetype_id: notetype,
        fields: vec![front.into(), "back".into()],
        tags: Vec::new(),
        deck_id: 1,
        guid: None,
        update_add_defaults: false,
    })
    .unwrap();
    (added.id, added.cards[0])
}

fn done(rx: &mpsc::Receiver<CoreEvent>, start: meguri_ffi::bulk_actions::BulkActionJobStart) -> BulkActionResult {
    assert_eq!(finish(rx, start.handle.id), JobOutcome::Succeeded);
    start.result.result().unwrap().expect("the result is set before JobFinished")
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

#[test]
fn bulk_actions_run_as_jobs_with_their_counts_and_steps() {
    let dir = tempfile::tempdir().unwrap();
    let rx = open(&dir);
    let notetype = add_notetype(AddNotetypeRequest {
        name: "Duo".into(),
        cloze: false,
        fields: vec!["Front".into(), "Back".into()],
        templates: vec![template("Forward", "{{Front}}")],
        css: Some(String::new()),
    })
    .unwrap()
    .id;
    let (n1, c1) = note(notetype, "one");
    let (n2, c2) = note(notetype, "two");

    let moved = done(
        &rx,
        start_bulk_card_action(vec![c1, c2], BulkCardAction::ChangeDeckToName { name: "Quorp::Zib".into() }).unwrap(),
    );
    assert_eq!((moved.affected, moved.considered, moved.cards_removed), (2, 2, None));
    assert_eq!(moved.summary.undo_name.as_deref(), Some("Change Deck"));

    let suspended = done(&rx, start_bulk_card_action(vec![c1, c2], BulkCardAction::Suspend).unwrap());
    assert_eq!((suspended.affected, suspended.summary.undo_name.as_deref()), (2, Some("Suspend")));
    let restored = done(&rx, start_bulk_card_action(vec![c1], BulkCardAction::Unbury).unwrap());
    assert_eq!((restored.affected, restored.considered), (0, 1), "Unbury leaves a suspended card");
    let restored = done(&rx, start_bulk_card_action(vec![c1], BulkCardAction::Unsuspend).unwrap());
    assert_eq!((restored.affected, restored.summary.undo_name.as_deref()), (1, Some("Unbury/Unsuspend")));
    let flagged = done(&rx, start_bulk_card_action(vec![c1, c2], BulkCardAction::SetFlag { flag: 3 }).unwrap());
    assert_eq!((flagged.affected, flagged.summary.undo_name.as_deref()), (2, Some("Set Flag")));

    let refused = start_bulk_card_action(Vec::new(), BulkCardAction::Bury).unwrap();
    assert!(matches!(
        finish(&rx, refused.handle.id),
        JobOutcome::Failed {
            error: CoreError::InvalidArgument { .. }
        }
    ));
    assert!(matches!(refused.result.result(), Err(CoreError::InvalidArgument { .. })));

    let tagged = done(
        &rx,
        start_bulk_note_action(vec![n1, n2], BulkNoteAction::AddTags { text: "red\tblue".into() }).unwrap(),
    );
    assert_eq!((tagged.affected, tagged.summary.undo_name.as_deref()), (2, Some("Update Tag")));
    let deleted = done(&rx, start_bulk_note_action(vec![n1], BulkNoteAction::DeleteNotes).unwrap());
    assert_eq!((deleted.affected, deleted.considered, deleted.cards_removed), (1, 1, Some(1)));
    assert_eq!(deleted.summary.undo_name.as_deref(), Some("Delete Note"));
}

/// What an action did, without the commit's seq: the counts, the step and
/// the rows it changed.
fn effect(result: &BulkActionResult) -> impl PartialEq + std::fmt::Debug {
    (
        result.affected,
        result.considered,
        result.cards_removed,
        result.summary.undo_name.clone(),
        result.summary.changes.clone(),
    )
}

fn undo_last(rx: &mpsc::Receiver<CoreEvent>) {
    let undo = start_undo(ReviewUndoScope::Global).unwrap();
    assert_eq!(finish(rx, undo.handle.id), JobOutcome::Succeeded);
}

/// K1, B1, H12: an action on a selection resolves it in the job and does
/// what the id variant does on the ids the selection resolves to; a card
/// action on a Notes-mode selection acts on every card of each selected
/// note; a selection that cannot be resolved is the job's
/// `InvalidArgument`.
#[test]
fn selection_variants_act_as_the_id_variants_on_the_same_rows() {
    let dir = tempfile::tempdir().unwrap();
    let rx = open(&dir);
    let notetype = add_notetype(AddNotetypeRequest {
        name: "Twin".into(),
        cloze: false,
        fields: vec!["Front".into(), "Back".into()],
        templates: vec![template("Forward", "{{Front}}"), template("Reverse", "{{Back}}")],
        css: Some(String::new()),
    })
    .unwrap()
    .id;
    for front in ["arn", "bex", "cyl"] {
        note(notetype, front);
    }
    let (tx, events) = mpsc::channel();
    let session = open_search_session(Arc::new(Searches(tx))).unwrap();
    let search = |mode: BrowseMode| {
        let seq = session
            .start_search(
                String::new(),
                mode,
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
    };
    let range = |start, end| vec![BrowseIndexRange { start, end }];

    // Cards mode: the first three rows (arn's two cards and one of bex's).
    let cards = search(BrowseMode::Cards);
    let rows = session.selection(cards, 0, range(0, 3)).unwrap();
    let ids = rows.resolve(BrowseMode::Cards).unwrap();
    assert_eq!(ids.len(), 3);
    let vond = add_deck("Vond".into()).unwrap().id;
    for action in [
        BulkCardAction::Suspend,
        BulkCardAction::SetFlag { flag: 2 },
        BulkCardAction::ChangeDeck { deck: vond },
    ] {
        let on_rows = done(
            &rx,
            start_bulk_card_action_on_selection(Arc::clone(&rows), action.clone()).unwrap(),
        );
        undo_last(&rx);
        let on_ids = done(&rx, start_bulk_card_action(ids.clone(), action.clone()).unwrap());
        undo_last(&rx);
        assert_eq!(effect(&on_rows), effect(&on_ids), "{action:?}");
        assert_eq!((on_rows.affected, on_rows.considered), (3, 3), "{action:?}");
    }

    // Notes mode: a card action acts on every card of each selected note.
    let notes = search(BrowseMode::Notes);
    let first = session.selection(notes, 0, range(0, 2)).unwrap();
    let note_ids = first.resolve(BrowseMode::Notes).unwrap();
    let their_cards = first.resolve(BrowseMode::Cards).unwrap();
    assert_eq!((note_ids.len(), their_cards.len()), (2, 4));
    let on_notes = done(
        &rx,
        start_bulk_card_action_on_selection(Arc::clone(&first), BulkCardAction::Bury).unwrap(),
    );
    undo_last(&rx);
    let on_cards = done(&rx, start_bulk_card_action(their_cards, BulkCardAction::Bury).unwrap());
    undo_last(&rx);
    assert_eq!(effect(&on_notes), effect(&on_cards));
    assert_eq!((on_notes.affected, on_notes.considered), (4, 4));

    // Note actions: on a Notes-mode selection, and on a Cards-mode one,
    // whose notes are those of its rows.
    for (selection, expected) in [(Arc::clone(&first), note_ids.clone()), (Arc::clone(&rows), note_ids.clone())] {
        for action in [BulkNoteAction::AddTags { text: "zeph".into() }, BulkNoteAction::DeleteNotes] {
            let on_rows = done(
                &rx,
                start_bulk_note_action_on_selection(Arc::clone(&selection), action.clone()).unwrap(),
            );
            undo_last(&rx);
            let on_ids = done(&rx, start_bulk_note_action(expected.clone(), action.clone()).unwrap());
            undo_last(&rx);
            assert_eq!(effect(&on_rows), effect(&on_ids), "{action:?}");
            assert_eq!(on_rows.considered, 2, "{action:?}");
        }
    }
    assert!(read_note(note_ids[0]).unwrap().tags.is_empty(), "every step was undone");

    // A selection of another revision is refused when the job resolves it.
    // One job at a time: `finish` drops the events of other jobs.
    let stale = session.selection(cards, 9, range(0, 1)).unwrap();
    let starts: [Box<dyn Fn() -> meguri_ffi::bulk_actions::BulkActionJobStart>; 2] = [
        Box::new(|| start_bulk_card_action_on_selection(Arc::clone(&stale), BulkCardAction::Suspend).unwrap()),
        Box::new(|| start_bulk_note_action_on_selection(Arc::clone(&stale), BulkNoteAction::DeleteNotes).unwrap()),
    ];
    for start in starts.iter().map(|start| start()) {
        assert!(matches!(
            finish(&rx, start.handle.id),
            JobOutcome::Failed {
                error: CoreError::InvalidArgument { .. }
            }
        ));
        assert!(matches!(start.result.result(), Err(CoreError::InvalidArgument { .. })));
    }
    session.close().unwrap();
}
