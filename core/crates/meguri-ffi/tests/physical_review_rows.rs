//! One process owns the real exported runtime. Expected fields are independently
//! read from the stored rows, not inferred from tracking or a synthetic receipt.
use meguri_engine::{OpSource, OpSpec};
use meguri_ffi::{
    CoreEvent, CoreEventKind, CoreEventListener, JobOutcome,
    collection::{self, CollectionLocation, DiskCapacity, DiskSpaceProvider, HostClock},
    physical_review_rows::PhysicalReviewCell,
    test_hooks::{collection_for_testing, test_physical_review_rows},
};
use meguri_model::{
    card::{CardQueue, CardType},
    card_data::CardData,
    clock::{FixedClock, UtcOffset},
    ids::{DeckId, RevlogId},
    revlog::{RevlogEntry, RevlogKind},
    rng::SeededRandom,
    usn::Usn,
};
use meguri_storage::{Cell, ReadRows, ReaderRole, RowKey, Table};
use std::{
    sync::{Arc, mpsc},
    time::Duration,
};
const NOW: i64 = 1_735_689_600_000;
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
fn assert_cells(actual: &[PhysicalReviewCell], stored: &[Cell]) {
    assert_eq!(actual.len(), stored.len());
    for (actual, stored) in actual.iter().zip(stored) {
        match (actual, stored) {
            (PhysicalReviewCell::Null, Cell::Null) => {}
            (PhysicalReviewCell::Integer { value }, Cell::Integer(stored)) => assert_eq!(value, stored),
            (PhysicalReviewCell::Real { bits }, Cell::Real(stored)) => assert_eq!(*bits, stored.to_bits()),
            (PhysicalReviewCell::Text { bytes }, Cell::Text(stored))
            | (PhysicalReviewCell::Blob { bytes }, Cell::Blob(stored)) => assert_eq!(bytes, stored),
            _ => panic!("observer changed a storage class"),
        }
    }
}
#[test]
fn physical_observer_matches_actual_card_history_and_one_snapshot_without_mutation() {
    let dir = tempfile::tempdir().unwrap();
    let collection_dir = dir.path().join("collection");
    let backups = dir.path().join("backups");
    std::fs::create_dir_all(&collection_dir).unwrap();
    std::fs::create_dir_all(&backups).unwrap();
    meguri_engine::bootstrap::create(
        &collection_dir.join("collection.anki2"),
        &FixedClock::new(NOW, UtcOffset::UTC),
        &SeededRandom::new(113),
    )
    .unwrap();
    let (tx, rx) = mpsc::channel();
    meguri_ffi::events::set_event_listener(Arc::new(Listener(tx))).unwrap();
    let job = collection::start_open(
        CollectionLocation {
            collection_directory: collection_dir.to_string_lossy().into_owned(),
            backups_directory: backups.to_string_lossy().into_owned(),
            app_group_snapshot: dir.path().join("widget.snapshot").to_string_lossy().into_owned(),
            manual_transfer_directory: dir.path().join("manual").to_string_lossy().into_owned(),
        },
        Arc::new(Clock),
        Arc::new(Space),
        113,
    )
    .unwrap();
    loop {
        let event = rx.recv_timeout(Duration::from_secs(20)).unwrap();
        if let CoreEventKind::JobFinished { id, outcome } = event.kind
            && id == job.id
        {
            assert_eq!(outcome, JobOutcome::Succeeded);
            break;
        }
    }
    let collection = collection_for_testing().unwrap();
    let nt = {
        let reader = collection.reader(ReaderRole::Query).unwrap();
        meguri_engine::notetypes::read::by_name(&*reader, "Basic")
            .unwrap()
            .unwrap()
            .id
    };
    let note = meguri_engine::notes::add(
        &collection,
        meguri_engine::notes::AddNote {
            notetype_id: nt,
            fields: vec!["observer front".into(), "observer back".into()],
            tags: vec![],
            deck_id: DeckId(1),
            guid: None,
            update_add_defaults: false,
        },
        OpSource::UserUndoable,
    )
    .unwrap()
    .value;
    let card_id = collection
        .reader(ReaderRole::Query)
        .unwrap()
        .cards_of_note(note.id)
        .unwrap()[0]
        .id;
    let revlog_id = RevlogId(NOW);
    collection.run(OpSpec::named("physical observer fixture", OpSource::Maintenance), move |ctx| {
        let mut card = ctx.scope().card(card_id)?.unwrap();
        card.card_type = CardType::Review; card.queue = CardQueue::Review;
        card.due = 39; card.interval = 17; card.ease_factor = 2420;
        card.reps = 11; card.lapses = 2; card.remaining_steps = 3;
        card.original_due = -15; card.original_deck_id = DeckId(1); card.flags = 5;
        card.data = CardData::parse(r#"{"s":23.125,"d":4.25,"dr":0.9,"decay":0.5,"lrt":1735689500,"cd":"{\"synthetic\":1}","unknown":[1,2]}"#);
        ctx.scope().put_card_forced(&card)?;
        ctx.scope().insert_revlog(&RevlogEntry {
            id: revlog_id, card_id, usn: Usn(-1), button: 4, interval: 17,
            last_interval: -60, ease_factor: 980, answer_millis: 543, kind: RevlogKind::Review,
        })?;
        Ok(())
    }).unwrap();
    meguri_ffi::test_hooks::engine_for_testing()
        .unwrap()
        .maintenance()
        .wait_idle_for_testing();
    let before = collection.last_seq();
    let observed = test_physical_review_rows(card_id.0, revlog_id.0).unwrap();
    // Independent expected snapshot uses the original collection, not the DTO mapper.
    {
        let reader = collection.reader(ReaderRole::Query).unwrap();
        let snapshot = reader.snapshot().unwrap();
        assert_eq!(observed.generation, snapshot.generation().0);
        assert_eq!(observed.seq, snapshot.seq().0);
        let card = snapshot.card(card_id).unwrap().unwrap();
        let revlog = snapshot.revlog_entry(revlog_id).unwrap().unwrap();
        let got_card = observed.card.as_ref().unwrap();
        let got_revlog = observed.revlog.as_ref().unwrap();
        assert_eq!(got_card.id, card.id.0);
        assert_eq!(got_revlog.id, revlog.id.0);
        assert_eq!(got_card.cells.len(), 18);
        assert_eq!(got_revlog.cells.len(), 9);
        assert_eq!(got_card.cells[0], PhysicalReviewCell::Integer { value: card.id.0 });
        assert_eq!(got_revlog.cells[0], PhysicalReviewCell::Integer { value: revlog.id.0 });
        assert_eq!(
            got_revlog.cells[1],
            PhysicalReviewCell::Integer {
                value: revlog.card_id.0
            }
        );
        assert_eq!(revlog.card_id, card.id);
        // Every stored cell replaces the shadow DTO's typed-field comparisons,
        // retaining even values a typed reader would normalize (R4/R5).
        assert_cells(
            &got_card.cells,
            &snapshot
                .row_image(Table::Cards, &RowKey::Id(card_id.0))
                .unwrap()
                .unwrap(),
        );
        assert_cells(
            &got_revlog.cells,
            &snapshot
                .row_image(Table::Revlog, &RowKey::Id(revlog_id.0))
                .unwrap()
                .unwrap(),
        );
    }
    let missing = test_physical_review_rows(i64::MAX, i64::MAX).unwrap();
    assert!(missing.card.is_none() && missing.revlog.is_none());
    let one_missing = test_physical_review_rows(card_id.0, i64::MAX).unwrap();
    assert_eq!(one_missing.card, observed.card);
    assert!(one_missing.revlog.is_none());
    assert_eq!(collection.last_seq(), before, "observations published no commit");
    assert_eq!(test_physical_review_rows(card_id.0, revlog_id.0).unwrap(), observed);
    meguri_ffi::lifecycle::close_collection().unwrap();
    assert!(test_physical_review_rows(card_id.0, revlog_id.0).is_err());
    assert!(meguri_ffi::physical_review_rows::observe(&collection, card_id.0, revlog_id.0).is_err());
}
