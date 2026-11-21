//! U87's bulk actions through their real exports on an opened collection:
//! Change Deck to a typed name, the scheduling actions admitted by the
//! review service, a refused empty list as the job's failure, the tag
//! actions and Delete Notes with its second count.
use meguri_ffi::{
    CoreError, CoreEvent, CoreEventKind, CoreEventListener, JobOutcome,
    bulk_actions::{BulkActionResult, BulkCardAction, BulkNoteAction, start_bulk_card_action, start_bulk_note_action},
    collection::{self, CollectionLocation, DiskCapacity, DiskSpaceProvider, HostClock},
    notes::{AddNoteRequest, add_note},
    notetypes::{AddNotetypeRequest, NewNotetypeTemplate, add_notetype},
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

#[test]
fn bulk_actions_run_as_jobs_with_their_counts_and_steps() {
    let dir = tempfile::tempdir().unwrap();
    let rx = open(&dir);
    let notetype = add_notetype(AddNotetypeRequest {
        name: "Duo".into(),
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
