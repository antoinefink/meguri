//! One test owns this binary's process-global listener and collection runtime.
use meguri_engine::{OpSource, OpSpec};
use meguri_ffi::{
    CoreError, CoreEvent, CoreEventKind, CoreEventListener, JobOutcome,
    collection::{self, CollectionLocation, DiskCapacity, DiskSpaceProvider, HostClock},
    deck_tree,
    events::set_event_listener,
    lifecycle::{self, CollectionLifecycle},
    review_session,
};
use meguri_model::{
    clock::{FixedClock, UtcOffset},
    rng::SeededRandom,
};
use meguri_storage::{ConfigValue, ReadRows, ReaderRole, space::StatvfsSpace};
use std::{
    path::Path,
    sync::{
        Arc, Barrier, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
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
struct MutableSpace(Arc<AtomicBool>);
impl DiskSpaceProvider for MutableSpace {
    fn available_bytes(&self, _: String) -> DiskCapacity {
        DiskCapacity::Available {
            bytes: if self.0.load(Ordering::SeqCst) { 0 } else { u64::MAX },
        }
    }
}
struct Listener(mpsc::Sender<CoreEvent>);
impl CoreEventListener for Listener {
    fn on_event(&self, event: CoreEvent) {
        let _ = self.0.send(event);
    }
}
fn finish(rx: &mpsc::Receiver<CoreEvent>, id: u64) -> (JobOutcome, Vec<CoreEvent>) {
    let mut seen = Vec::new();
    loop {
        let event = rx.recv_timeout(Duration::from_secs(20)).expect("open completion");
        let outcome = match &event.kind {
            CoreEventKind::JobFinished { id: got, outcome } if *got == id => Some(outcome.clone()),
            _ => None,
        };
        seen.push(event);
        if let Some(outcome) = outcome {
            return (outcome, seen);
        }
    }
}
fn start_review(
    rx: &mpsc::Receiver<CoreEvent>,
    events: &mut Vec<CoreEvent>,
    deck: i64,
) -> review_session::ReviewStartResult {
    let job = review_session::start_session(deck, None).unwrap();
    let (outcome, more) = finish(rx, job.handle.id);
    events.extend(more);
    assert_eq!(outcome, JobOutcome::Succeeded);
    job.result.result().unwrap().expect("completed start result")
}
fn end_review(rx: &mpsc::Receiver<CoreEvent>, events: &mut Vec<CoreEvent>, id: u64) {
    let job = review_session::end_session(id).unwrap();
    let (outcome, more) = finish(rx, job.id);
    events.extend(more);
    assert_eq!(outcome, JobOutcome::Succeeded);
}
fn location(root: &Path) -> CollectionLocation {
    let collection = root.join("collection");
    let backups = root.join("backups");
    std::fs::create_dir_all(&collection).unwrap();
    std::fs::create_dir_all(&backups).unwrap();
    CollectionLocation {
        manual_transfer_directory: root.join("ManualTransfers").to_string_lossy().into_owned(),
        collection_directory: collection.to_str().unwrap().into(),
        backups_directory: backups.to_str().unwrap().into(),
        app_group_snapshot: root.join("widget.snapshot").to_str().unwrap().into(),
    }
}
fn fixture(location: &CollectionLocation) {
    let clock = Arc::new(FixedClock::new(NOW, UtcOffset::UTC));
    meguri_engine::bootstrap::create(
        &Path::new(&location.collection_directory).join("collection.anki2"),
        clock.as_ref(),
        &SeededRandom::new(37),
    )
    .unwrap();
    let host = meguri_host::CoreHost::open(meguri_host::HostConfig::new(
        location.collection_directory.clone().into(),
        location.backups_directory.clone().into(),
        location.app_group_snapshot.clone().into(),
        clock,
        Arc::new(SeededRandom::new(37)),
        Arc::new(StatvfsSpace),
    ))
    .unwrap();
    let c = host.collection().unwrap();
    c.run(OpSpec::named("fixture policy", OpSource::Maintenance), |ctx| {
        ctx.scope().set_config("fsrs", &ConfigValue::Bool(true))?;
        ctx.scope()
            .set_config("loadBalancerEnabled", &ConfigValue::Bool(false))?;
        Ok(())
    })
    .unwrap();
    let zoo = meguri_engine::decks::add(&c, "Zoo".into()).unwrap().value.id;
    let child = meguri_engine::decks::add(&c, "alpha::Child".into()).unwrap().value.id;
    let nt = {
        let reader = c.reader(ReaderRole::Query).unwrap();
        meguri_engine::notetypes::read::by_name(&*reader, "Basic")
            .unwrap()
            .unwrap()
            .id
    };
    for (index, deck) in [child, child, zoo].into_iter().enumerate() {
        meguri_engine::notes::add(
            &c,
            meguri_engine::notes::AddNote {
                notetype_id: nt,
                fields: vec![format!("front {index}"), "back".into()],
                tags: vec![],
                deck_id: deck,
                guid: None,
                update_add_defaults: false,
            },
            OpSource::UserUndoable,
        )
        .unwrap();
    }
    c.run(
        OpSpec::named("fixture description", OpSource::Maintenance),
        move |ctx| {
            let mut deck = ctx.scope().deck(zoo)?.unwrap();
            if let meguri_model::proto::deck::DeckKind::Normal(normal) = &mut deck.kind.kind {
                normal.description = "<b>safe</b><script>unsafe()</script>".into();
            }
            meguri_engine::decks::save_in(ctx.scope(), deck)?;
            Ok(())
        },
    )
    .unwrap();
    c.run(
        OpSpec::named("fixture imported sentinels", OpSource::Maintenance),
        move |ctx| {
            fn unknown<M: meguri_model::proto::BlobMessage>(
                b: &meguri_model::proto::ProtoBlob<M>,
            ) -> meguri_model::proto::ProtoBlob<M> {
                let mut bytes = b.to_bytes().into_owned();
                bytes.extend_from_slice(&[0xa0, 0x01, 0x07]);
                meguri_model::proto::ProtoBlob::decode(&bytes).unwrap()
            }
            let mut n = ctx.scope().notetype(nt)?.unwrap();
            n.config.latex_pre = "imported latex preamble".into();
            n.config.latex_svg = true;
            n.config = unknown(&n.config);
            n.fields[0].config = unknown(&n.fields[0].config);
            n.templates[0].config = unknown(&n.templates[0].config);
            ctx.scope().put_notetype(&n)?;
            for mut note in ctx.scope().notes_after(None, 100)? {
                note.flags = 64;
                note.data = "imported note data".into();
                ctx.scope().put(&note)?;
            }
            for mut card in ctx.scope().cards_after(None, 100)? {
                card.flags = 0xa0;
                ctx.scope().put(&card)?;
            }
            Ok(())
        },
    )
    .unwrap();
    host.close().unwrap();
}
struct BlockingClock {
    first: AtomicBool,
    entered: mpsc::Sender<()>,
    release: Mutex<mpsc::Receiver<()>>,
}
impl HostClock for BlockingClock {
    fn now_ms(&self) -> i64 {
        if !self.first.swap(true, Ordering::SeqCst) {
            self.entered.send(()).unwrap();
            self.release.lock().unwrap().recv().unwrap();
        }
        NOW
    }
    fn offset_from_utc(&self, _: i64) -> i32 {
        0
    }
    fn zone_id(&self) -> String {
        "UTC".into()
    }
}
struct InvalidOffsetClock;
impl HostClock for InvalidOffsetClock {
    fn now_ms(&self) -> i64 {
        NOW
    }
    fn offset_from_utc(&self, _: i64) -> i32 {
        86_400
    }
    fn zone_id(&self) -> String {
        "UTC".into()
    }
}
#[test]
fn exported_open_counts_review_close_reopen_upgrade_and_races() {
    let dir = tempfile::tempdir().unwrap();
    let location = location(dir.path());
    fixture(&location);
    let (tx, rx) = mpsc::channel();
    set_event_listener(Arc::new(Listener(tx))).unwrap();
    let low_space = Arc::new(AtomicBool::new(false));
    let open = || {
        collection::start_open(
            location.clone(),
            Arc::new(Clock),
            Arc::new(MutableSpace(low_space.clone())),
            37,
        )
        .unwrap()
    };
    let first = open();
    let (outcome, mut events) = finish(&rx, first.id);
    assert_eq!(outcome, JobOutcome::Succeeded);
    assert_eq!(
        lifecycle::collection_lifecycle().unwrap(),
        CollectionLifecycle::Ready { generation: 1 }
    );
    assert_eq!(lifecycle::wait_collection_available(0).unwrap(), 1);
    assert!(events.iter().any(|e| e.generation == 1
        && e.kind
            == CoreEventKind::Lifecycle {
                state: CollectionLifecycle::Ready { generation: 1 }
            }));
    let tree = deck_tree::deck_tree_with_counts().unwrap();
    assert_eq!(tree.generation, 1);
    assert_eq!(tree.next_change_at, 1_894_766_400);
    assert_eq!(
        tree.nodes.iter().map(|n| n.name.as_str()).collect::<Vec<_>>(),
        ["alpha", "Zoo"]
    );
    assert_eq!(
        (
            tree.nodes[0].counts.new,
            tree.nodes[0].counts.learning,
            tree.nodes[0].counts.review
        ),
        (2, 0, 0)
    );
    assert_eq!(tree.nodes[0].children[0].name, "Child");
    assert_eq!(tree.nodes[0].children[0].counts.new, 2);
    assert_eq!(tree.nodes[1].counts.new, 1);
    assert_eq!(
        deck_tree::deck_description_html(tree.nodes[1].id).unwrap(),
        "<b>safe</b>"
    );
    assert_eq!(
        deck_tree::deck_study_state(tree.nodes[0].id).unwrap().queue_counts.new,
        2
    );
    let persisted = typed_surface(&rx, &mut events, tree.nodes[1].id, &low_space);
    let review_session::ReviewStartResult::Started {
        session_id,
        generation,
        head,
        ..
    } = start_review(&rx, &mut events, tree.nodes[0].id)
    else {
        panic!("review must use the opened host")
    };
    assert_eq!(generation, 1);
    assert_eq!(head.counts.new, 2);
    assert!(!head.items.is_empty());
    end_review(&rx, &mut events, session_id);
    let review_session::ReviewStartResult::Started { session_id, .. } =
        start_review(&rx, &mut events, tree.nodes[0].id)
    else {
        panic!("close fixture must start")
    };
    // Closing an active session must really drain and end it before success.
    lifecycle::close_collection().unwrap();
    assert!(review_session::session_head(session_id, 3).is_err());
    assert_eq!(
        lifecycle::collection_lifecycle().unwrap(),
        CollectionLifecycle::NoCollection
    );
    assert!(deck_tree::deck_tree_with_counts().is_err());
    let second = open();
    let (outcome, more) = finish(&rx, second.id);
    events.extend(more);
    assert_eq!(outcome, JobOutcome::Succeeded);
    assert_eq!(deck_tree::deck_tree_with_counts().unwrap().generation, 2);
    assert_eq!(
        meguri_ffi::notes::read_note(persisted.0).unwrap().fields,
        ["persisted front", "back"]
    );
    assert_eq!(
        meguri_ffi::notetypes::read_notetype(persisted.1).unwrap().css,
        ".card { color: green; }"
    );
    assert_eq!(
        meguri_ffi::media::add_media("Surface.PNG".into(), vec![1, 2, 3]).unwrap(),
        "surface.png"
    );

    lifecycle::close_collection().unwrap();
    // Hold an actual foreign clock call. Both competing control operations
    // must refuse promptly, and neither may close this open's generation.
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let pending = collection::start_open(
        location.clone(),
        Arc::new(BlockingClock {
            first: AtomicBool::new(false),
            entered: entered_tx,
            release: Mutex::new(release_rx),
        }),
        Arc::new(Space),
        37,
    )
    .unwrap();
    entered_rx.recv_timeout(Duration::from_secs(10)).unwrap();
    assert!(matches!(lifecycle::close_collection(), Err(CoreError::Busy { .. })));
    assert!(matches!(
        collection::start_open(location.clone(), Arc::new(Clock), Arc::new(Space), 37),
        Err(CoreError::Busy { .. })
    ));
    pending.cancel.cancel().unwrap();
    release_tx.send(()).unwrap();
    let (outcome, more) = finish(&rx, pending.id);
    events.extend(more);
    assert!(
        matches!(outcome, JobOutcome::Failed { error: CoreError::Unavailable { reason } } if reason.contains("remains open"))
    );
    assert_eq!(
        lifecycle::collection_lifecycle().unwrap(),
        CollectionLifecycle::Ready { generation: 3 }
    );
    lifecycle::close_collection().unwrap();
    // A queued cancelled open releases ownership before JobFinished, even
    // when the registry skips its work closure entirely.
    let workers = meguri_engine::EngineConfig::default().job_workers;
    let started = Arc::new(Barrier::new(workers + 1));
    let release = Arc::new(Barrier::new(workers + 1));
    for _ in 0..workers {
        let started = started.clone();
        let release = release.clone();
        meguri_ffi::jobs::start("occupy pool", 0, move |_| {
            started.wait();
            release.wait();
            Ok(())
        })
        .unwrap();
    }
    started.wait();
    let queued = open();
    queued.cancel.cancel().unwrap();
    release.wait();
    let (outcome, more) = finish(&rx, queued.id);
    events.extend(more);
    assert_eq!(outcome, JobOutcome::Cancelled);
    assert_eq!(
        lifecycle::collection_lifecycle().unwrap(),
        CollectionLifecycle::NoCollection
    );
    let next = open();
    let (outcome, more) = finish(&rx, next.id);
    events.extend(more);
    assert_eq!(outcome, JobOutcome::Succeeded);
    lifecycle::close_collection().unwrap();
    // A bad synchronous host callback unwinds an open step. No live writer
    // or poisoned process reservation may escape to prevent a successor.
    let invalid = collection::start_open(location.clone(), Arc::new(InvalidOffsetClock), Arc::new(Space), 37).unwrap();
    let (outcome, more) = finish(&rx, invalid.id);
    events.extend(more);
    assert!(matches!(outcome, JobOutcome::Failed { .. }));
    let next = open();
    let (outcome, more) = finish(&rx, next.id);
    events.extend(more);
    assert_eq!(outcome, JobOutcome::Succeeded);
    lifecycle::close_collection().unwrap();
    // Existing generated schema-11 fixture, copied with no newly invented input.
    let upgraded = tempfile::tempdir().unwrap();
    let upgrade = self::location(upgraded.path());
    std::fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../conformance/fixtures/storage/schema-11.anki2"),
        Path::new(&upgrade.collection_directory).join("collection.anki2"),
    )
    .unwrap();
    let job = collection::start_open(upgrade, Arc::new(Clock), Arc::new(Space), 37).unwrap();
    let (outcome, more) = finish(&rx, job.id);
    events.extend(more);
    assert_eq!(outcome, JobOutcome::Succeeded);
    assert!(events.iter().any(|e| matches!(
        e.kind,
        CoreEventKind::Lifecycle {
            state: CollectionLifecycle::Upgrading { from: 11, .. }
        }
    )));
    lifecycle::close_collection().unwrap();
    // An error before/inside open must release the reservation for a successor.
    let missing = tempfile::tempdir().unwrap();
    let missing_location = self::location(missing.path());
    let failed = collection::start_open(missing_location, Arc::new(Clock), Arc::new(Space), 37).unwrap();
    let (outcome, more) = finish(&rx, failed.id);
    events.extend(more);
    assert!(matches!(
        outcome,
        JobOutcome::Failed {
            error: CoreError::Open {
                failure: lifecycle::CollectionOpenFailure::NotFound
            }
        }
    ));
    assert!(matches!(
        lifecycle::collection_lifecycle().unwrap(),
        CollectionLifecycle::Failed {
            reason: lifecycle::CollectionFailedReason::Open {
                error: lifecycle::CollectionOpenFailure::NotFound
            }
        }
    ));
    let last = open();
    let (outcome, more) = finish(&rx, last.id);
    events.extend(more);
    assert_eq!(outcome, JobOutcome::Succeeded);
    lifecycle::close_collection().unwrap();
    verify_imported_sentinels(&location, persisted.1);
    #[cfg(feature = "test-hooks")]
    all_deck_name_projection_regressions(&rx, &mut events);
    assert!(
        events.windows(2).all(|e| e[0].seq < e[1].seq),
        "one process dispatcher preserves order across reopen"
    );
}

#[cfg(feature = "test-hooks")]
fn all_deck_name_projection_regressions(rx: &mpsc::Receiver<CoreEvent>, events: &mut Vec<CoreEvent>) {
    use meguri_ffi::decks::all_deck_names;
    use meguri_model::{
        ids::DeckId,
        proto::{
            ProtoBlob,
            deck::{DeckKind, DeckKindMessage, FilteredDeck},
        },
    };
    let dir = tempfile::tempdir().unwrap();
    let location = self::location(dir.path());
    meguri_engine::bootstrap::create(
        &Path::new(&location.collection_directory).join("collection.anki2"),
        &FixedClock::new(NOW, UtcOffset::UTC),
        &SeededRandom::new(37),
    )
    .unwrap();
    let open = |events: &mut Vec<CoreEvent>| {
        let job = collection::start_open(location.clone(), Arc::new(Clock), Arc::new(Space), 37).unwrap();
        let (outcome, more) = finish(rx, job.id);
        events.extend(more);
        assert_eq!(outcome, JobOutcome::Succeeded);
    };
    open(events);
    let c = meguri_ffi::test_hooks::collection_for_testing().unwrap();
    c.run(
        OpSpec::named("D17 persisted projection fixture", OpSource::Maintenance),
        |ctx| {
            for (id, name) in [
                (1, "Personal"),
                (-7, "alpha"),
                (7, "alpha\u{1f}É"),
                (8, "alpha\u{1f}É\u{1f}東京"),
                (9, "alpha?"),
                (12, "Missing\u{1f}Leaf"),
                (-9, "Search"),
                (13, " E\u{301} \u{1f} :猫: "),
            ] {
                // Raw typed rows model imported spelling without input repair.
                let mut deck = meguri_engine::decks::new_deck(name.into());
                deck.id = DeckId(id);
                if id == -9 {
                    deck.kind = ProtoBlob::new(DeckKindMessage::new(DeckKind::Filtered(FilteredDeck::default())));
                }
                ctx.scope().put(&deck)?;
            }
            Ok(())
        },
    )
    .unwrap();
    meguri_engine::decks::rename(&c, DeckId(9), "alpha!".into()).unwrap();
    meguri_engine::decks::rename(&c, DeckId(9), "alpha!!".into()).unwrap();
    c.undo(meguri_engine::UndoScope::Global).unwrap();
    {
        let reader = c.reader(ReaderRole::Query).unwrap();
        let tree = meguri_engine::decks::tree::tree(&*reader).unwrap();
        assert!(tree.iter().all(|node| node.id != DeckId(1)));
        assert!(tree.iter().all(|node| node.id != DeckId(12)));
        assert!(tree.iter().find(|node| node.id == DeckId(-7)).unwrap().study_collapsed);
    }
    fn state(c: &meguri_engine::Collection) -> Vec<String> {
        let reader = c.reader(ReaderRole::Query).unwrap();
        [
            "col",
            "decks",
            "cards",
            "notes",
            "revlog",
            "graves",
            "config",
            "deck_config",
            "notetypes",
            "fields",
            "templates",
            "tags",
        ]
        .iter()
        .map(|table| {
            format!(
                "{:?}",
                reader
                    .execute_raw(&format!("SELECT * FROM {table} ORDER BY 1"), &[])
                    .unwrap()
                    .rows
            )
        })
        .collect()
    }
    c.run(
        OpSpec::named("D17 settle rollover", OpSource::Rollover).stamp(meguri_storage::StampMode::Raw),
        |_| Ok(()),
    )
    .unwrap();
    let before = state(&c);
    let undo = c.peek_undo(meguri_engine::UndoScope::Global).unwrap();
    let redo = c.peek_redo(meguri_engine::UndoScope::Global).unwrap();
    assert!(matches!(undo, meguri_engine::UndoStatus::Available(_)));
    assert!(matches!(redo, meguri_engine::UndoStatus::Available(_)));
    let drain = meguri_ffi::jobs::start("D17 before read", c.generation().0, |_| Ok(())).unwrap();
    let (outcome, more) = finish(rx, drain.id);
    assert_eq!(outcome, JobOutcome::Succeeded);
    events.extend(more);
    let seq = c.last_seq();
    let names = all_deck_names().unwrap();
    assert_eq!(names.generation, c.generation().0);
    assert_eq!(
        names
            .decks
            .iter()
            .map(|d| (d.id, d.full_name.as_str(), d.filtered))
            .collect::<Vec<_>>(),
        [
            (13, " E\u{301} :: :猫: ", false),
            (-7, "alpha", false),
            (7, "alpha::É", false),
            (8, "alpha::É::東京", false),
            (9, "alpha!", false),
            (12, "Missing::Leaf", false),
            (1, "Personal", false),
            (-9, "Search", true),
        ]
    );
    assert_eq!(state(&c), before);
    assert_eq!(c.last_seq(), seq);
    assert_eq!(c.peek_undo(meguri_engine::UndoScope::Global).unwrap(), undo);
    assert_eq!(c.peek_redo(meguri_engine::UndoScope::Global).unwrap(), redo);
    let drain = meguri_ffi::jobs::start("D17 after read", c.generation().0, |_| Ok(())).unwrap();
    let (outcome, more) = finish(rx, drain.id);
    assert_eq!(outcome, JobOutcome::Succeeded);
    assert_eq!(
        more.len(),
        1,
        "the read publishes no events before the completion marker"
    );
    events.extend(more);
    // Search sorts after the valid rows, so a decode failure must discard
    // entries already read rather than return a successful prefix.
    let saved = {
        use meguri_storage::Record;
        let reader = c.reader(ReaderRole::Query).unwrap();
        reader.deck(DeckId(-9)).unwrap().unwrap().to_cells()
    };
    let mut malformed = saved.clone();
    malformed[5] = meguri_storage::Cell::Blob(vec![0xff]);
    c.run(
        OpSpec::named("D17 malformed deck fixture", OpSource::Maintenance).stamp(meguri_storage::StampMode::Raw),
        move |ctx| {
            ctx.scope().restore(
                meguri_storage::Table::Decks,
                &meguri_storage::RowKey::Id(-9),
                Some(&malformed),
            )?;
            Ok(())
        },
    )
    .unwrap();
    c.run(
        OpSpec::named("D17 settle malformed fixture", OpSource::Rollover).stamp(meguri_storage::StampMode::Raw),
        |_| Ok(()),
    )
    .unwrap();
    let before_failure = state(&c);
    let failure_undo = c.peek_undo(meguri_engine::UndoScope::Global).unwrap();
    let failure_redo = c.peek_redo(meguri_engine::UndoScope::Global).unwrap();
    let drain = meguri_ffi::jobs::start("D17 before failed read", c.generation().0, |_| Ok(())).unwrap();
    let (outcome, more) = finish(rx, drain.id);
    assert_eq!(outcome, JobOutcome::Succeeded);
    events.extend(more);
    let failure_seq = c.last_seq();
    assert!(matches!(all_deck_names(), Err(CoreError::Internal { .. })));
    assert_eq!(state(&c), before_failure);
    assert_eq!(c.last_seq(), failure_seq);
    assert_eq!(c.peek_undo(meguri_engine::UndoScope::Global).unwrap(), failure_undo);
    assert_eq!(c.peek_redo(meguri_engine::UndoScope::Global).unwrap(), failure_redo);
    let drain = meguri_ffi::jobs::start("D17 after failed read", c.generation().0, |_| Ok(())).unwrap();
    let (outcome, more) = finish(rx, drain.id);
    assert_eq!(outcome, JobOutcome::Succeeded);
    assert_eq!(more.len(), 1, "the failed read publishes no events");
    events.extend(more);
    c.run(
        OpSpec::named("D17 restore deck fixture", OpSource::Maintenance).stamp(meguri_storage::StampMode::Raw),
        move |ctx| {
            ctx.scope().restore(
                meguri_storage::Table::Decks,
                &meguri_storage::RowKey::Id(-9),
                Some(&saved),
            )?;
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(all_deck_names().unwrap(), names);
    let threads = meguri_engine::ThreadRegistry::default();
    let start = Arc::new(Barrier::new(2));
    let writer_start = start.clone();
    let writer_collection = c.clone();
    let writer = threads
        .spawn("ffi-deck-name-rename", meguri_engine::Qos::Default, move |_| {
            writer_start.wait();
            meguri_engine::decks::rename(&writer_collection, DeckId(-7), "beta".into()).unwrap();
        })
        .unwrap();
    start.wait();
    let racing = all_deck_names().unwrap();
    writer.join().unwrap();
    let paths = racing
        .decks
        .iter()
        .filter(|d| [-7, 7, 8].contains(&d.id))
        .map(|d| d.full_name.as_str())
        .collect::<Vec<_>>();
    assert!(paths == ["alpha", "alpha::É", "alpha::É::東京"] || paths == ["beta", "beta::É", "beta::É::東京"]);
    c.run(
        OpSpec::named("D17 absent default fixture", OpSource::Maintenance),
        |ctx| {
            ctx.scope().remove_deck(DeckId(1))?;
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(all_deck_names().unwrap().decks.len(), 7);
    assert!(
        all_deck_names()
            .unwrap()
            .decks
            .iter()
            .all(|d| d.id != 1 && d.full_name != "Default")
    );
    lifecycle::close_collection().unwrap();
    assert!(matches!(all_deck_names(), Err(CoreError::Unavailable { .. })));
    open(events);
    let current = all_deck_names().unwrap();
    assert_eq!(current.generation, names.generation + 1);
    assert_ne!(
        current.generation, names.generation,
        "completed old results remain old at publication"
    );
    assert!(
        matches!(c.reader(ReaderRole::Query), Err(meguri_engine::EngineError::StaleGeneration { planned, current: got })
            if planned.0 == names.generation && got.0 == current.generation)
    );
    let c = meguri_ffi::test_hooks::collection_for_testing().unwrap();
    c.run(OpSpec::named("D17 no records fixture", OpSource::Maintenance), |ctx| {
        for deck in ctx.scope().deck_rows()? {
            ctx.scope().remove_deck(deck.id)?;
        }
        Ok(())
    })
    .unwrap();
    assert!(all_deck_names().unwrap().decks.is_empty());
    lifecycle::close_collection().unwrap();
}

// Mirrors model ChangeSet::is_empty, including flags outside the row lists.
fn changes_empty(changes: &meguri_ffi::changes::ChangeSet) -> bool {
    !changes.whole_collection
        && changes.cards.is_empty()
        && changes.notes.is_empty()
        && changes.notetypes.is_empty()
        && changes.decks.is_empty()
        && changes.deck_configs.is_empty()
        && changes.config_keys.is_empty()
        && changes.tags.is_empty()
        && changes.revlog_added.is_empty()
        && changes.revlog_removed.is_empty()
        && changes.media_added.is_empty()
        && changes.media_removed.is_empty()
        && !changes.collection_row
}

// Runs within the single process-runtime test above, using only real exports.
fn typed_surface(
    rx: &mpsc::Receiver<CoreEvent>,
    events: &mut Vec<CoreEvent>,
    zoo: i64,
    low_space: &AtomicBool,
) -> (i64, i64) {
    use meguri_ffi::{decks, mutations::MutationSummary, notes, notetypes, suspend, tags, undo};
    use review_session::{ReviewUndoScope as Scope, ReviewUndoStatus as Status};
    let mut summaries: Vec<MutationSummary> = Vec::new();
    assert!(matches!(
        undo::peek_undo(Scope::Global).unwrap(),
        Status::Unavailable { .. }
    ));
    let no_undo = undo::start_undo(Scope::Global).unwrap();
    let (outcome, more) = finish(rx, no_undo.handle.id);
    events.extend(more);
    assert!(matches!(
        outcome,
        JobOutcome::Failed {
            error: CoreError::UndoUnavailable { .. }
        }
    ));
    assert!(matches!(
        notes::read_note(-99),
        Err(CoreError::NoteNotFound { id: -99 })
    ));
    #[cfg(feature = "test-hooks")]
    controlled_job_regressions(rx, events);
    let deck = decks::add_deck("Surface".into()).unwrap();
    summaries.push(deck.summary.clone());
    assert_eq!(deck.summary.undo_name.as_deref(), Some("Add Deck"));
    assert_eq!(deck.summary.generation, 1);
    assert!(deck.summary.changes.decks.iter().any(|r| r.id == deck.id));
    let rename_job = decks::start_rename_deck(deck.id, "Surface renamed".into()).unwrap();
    let (outcome, more) = finish(rx, rename_job.handle.id);
    events.extend(more);
    assert_eq!(outcome, JobOutcome::Succeeded);
    let renamed = rename_job.result.result().unwrap().unwrap();
    summaries.push(renamed.summary);
    assert_eq!(renamed.name, "Surface renamed");
    decks::add_deck("Surface renamed::Kid".into()).unwrap();
    let refusal = decks::start_rename_deck(deck.id, "Surface renamed::Kid::Parent".into()).unwrap();
    let (outcome, more) = finish(rx, refusal.handle.id);
    events.extend(more);
    assert!(matches!(
        outcome,
        JobOutcome::Failed {
            error: CoreError::DeckRefused {
                refusal: meguri_ffi::error_types::DeckRefusal::UnderDescendant
            }
        }
    ));
    let tree = deck_tree::deck_tree_with_counts().unwrap();
    let root = tree
        .nodes
        .iter()
        .find(|node| node.id == deck.id)
        .expect("renamed deck at root");
    assert_eq!(root.name, "Surface renamed");
    assert!(root.browser_collapsed, "deck {} should initially be collapsed", deck.id);
    let expanding = decks::start_collapse_decks(vec![deck.id], decks::DeckCollapseScope::Browser, false).unwrap();
    let (outcome, more) = finish(rx, expanding.handle.id);
    events.extend(more);
    assert_eq!(outcome, JobOutcome::Succeeded);
    let expanded = expanding.result.result().unwrap().unwrap();
    assert!(
        !changes_empty(&expanded.changes),
        "expand deck {}: {expanded:?}",
        deck.id
    );
    summaries.push(expanded);
    let collapsing = decks::start_collapse_decks(vec![deck.id], decks::DeckCollapseScope::Browser, false).unwrap();
    let (outcome, more) = finish(rx, collapsing.handle.id);
    events.extend(more);
    assert_eq!(outcome, JobOutcome::Succeeded);
    let collapsed = collapsing.result.result().unwrap().unwrap();
    assert!(
        changes_empty(&collapsed.changes),
        "collapse deck {}: {collapsed:?}",
        deck.id
    );
    summaries.push(collapsed);
    let moving = decks::start_move_decks(vec![deck.id], None).unwrap();
    let (outcome, more) = finish(rx, moving.handle.id);
    events.extend(more);
    assert_eq!(outcome, JobOutcome::Succeeded);
    let moved = moving.result.result().unwrap().unwrap();
    assert_eq!(moved.count, 0);
    assert!(
        changes_empty(&moved.summary.changes),
        "move root deck {}: {moved:?}",
        deck.id
    );
    summaries.push(moved.summary);

    let nt = notetypes::notetype_names()
        .unwrap()
        .into_iter()
        .find(|n| n.name == "Basic")
        .unwrap()
        .id;
    let before = notetypes::read_notetype(nt).unwrap();
    let css = notetypes::set_notetype_css(nt, before.base.clone(), ".card { color: green; }".into()).unwrap();
    summaries.push(css.summary);
    assert!(matches!(
        notetypes::rename_notetype(nt, before.base, "stale".into()),
        Err(CoreError::Stale { .. })
    ));
    let fresh = notetypes::read_notetype(nt).unwrap();
    let clone = notetypes::clone_notetype(nt, Some("Surface copy".into())).unwrap();
    assert_ne!(clone.id, nt);
    assert_eq!(notetypes::read_notetype(clone.id).unwrap().css, fresh.css);
    let bad = notetypes::add_notetype(notetypes::AddNotetypeRequest {
        name: "invalid".into(),
        cloze: false,
        fields: vec![],
        templates: vec![],
        css: None,
    });
    assert!(matches!(bad, Err(CoreError::NotetypeRefused { .. })));
    let note = notes::add_note(notes::AddNoteRequest {
        notetype_id: nt,
        fields: vec!["surface front".into(), "back".into()],
        tags: vec!["Parent::Child".into()],
        deck_id: deck.id,
        guid: Some("surface-guid".into()),
        update_add_defaults: false,
    })
    .unwrap();
    summaries.push(note.summary.clone());
    assert_eq!(note.cards.len(), 1);
    let check = notes::check_note_fields(nt, vec!["surface front".into(), "back".into()], None).unwrap();
    assert_eq!(check.state, notes::FieldState::Duplicate);
    assert_eq!(check.duplicates, [note.id]);
    assert_eq!(
        notes::check_note_fields(nt, vec!["surface front".into(), "back".into()], Some(note.id))
            .unwrap()
            .state,
        notes::FieldState::Normal
    );
    let updated = notes::update_note(notes::UpdateNoteRequest {
        id: note.id,
        fields: vec!["persisted front".into(), "back".into()],
        tags: vec!["Parent::Child".into()],
    })
    .unwrap();
    summaries.push(updated.summary);
    assert!(updated.written);
    assert!(
        !notes::update_note(notes::UpdateNoteRequest {
            id: note.id,
            fields: vec!["persisted front".into(), "back".into()],
            tags: vec!["Parent::Child".into()]
        })
        .unwrap()
        .written
    );
    assert_eq!(tags::mark_notes(vec![note.id]).unwrap().count, 1);
    assert!(notes::read_note(note.id).unwrap().tags.contains(&"marked".into()));
    assert_eq!(tags::toggle_note_mark(note.id, vec![note.id]).unwrap().count, 1);
    assert!(!notes::read_note(note.id).unwrap().tags.contains(&"marked".into()));
    assert_eq!(tags::add_note_tags(vec![note.id], "extra".into()).unwrap().count, 1);
    let job = tags::start_rename_tag("extra".into(), "renamed".into()).unwrap();
    let (outcome, more) = finish(rx, job.handle.id);
    events.extend(more);
    assert_eq!(outcome, JobOutcome::Succeeded);
    let tag_result = job.result.result().unwrap().unwrap();
    summaries.push(tag_result.summary);
    assert!(notes::read_note(note.id).unwrap().tags.contains(&"renamed".into()));
    let workers = meguri_engine::EngineConfig::default().job_workers;
    let entered = Arc::new(Barrier::new(workers + 1));
    let release = Arc::new(Barrier::new(workers + 1));
    for _ in 0..workers {
        let entered = entered.clone();
        let release = release.clone();
        meguri_ffi::jobs::start("hold bulk cancellation queue", 1, move |_| {
            entered.wait();
            release.wait();
            Ok(())
        })
        .unwrap();
    }
    entered.wait();
    let cancelled = tags::start_add_note_tags(vec![note.id], "must-not-appear".into()).unwrap();
    cancelled.handle.cancel.cancel().unwrap();
    release.wait();
    let (outcome, more) = finish(rx, cancelled.handle.id);
    events.extend(more);
    assert_eq!(outcome, JobOutcome::Cancelled);
    assert!(cancelled.result.result().unwrap().is_none());
    assert!(
        !notes::read_note(note.id)
            .unwrap()
            .tags
            .contains(&"must-not-appear".into())
    );
    // Flags declare long_write from the selection, and global undo/redo
    // retain that actual step data. No blocking undo runs on an FFI lane.
    let flag = tags::start_set_card_flag(vec![note.cards[0]; 2_001], 5).unwrap();
    let (outcome, more) = finish(rx, flag.handle.id);
    events.extend(more);
    assert_eq!(outcome, JobOutcome::Succeeded);
    let result = flag.result.result().unwrap().unwrap();
    assert_eq!(result.count, 1);
    summaries.push(result.summary);
    let Status::Available { step } = undo::peek_undo(Scope::Global).unwrap() else {
        panic!("flag undo missing")
    };
    assert!(step.long_write.is_some());
    let restore = undo::start_undo(Scope::Global).unwrap();
    let (outcome, more) = finish(rx, restore.handle.id);
    events.extend(more);
    assert_eq!(outcome, JobOutcome::Succeeded);
    let undo::UndoResult::Global { summary, step } = restore.result.result().unwrap().unwrap() else {
        panic!("global routed to session")
    };
    assert!(step.long_write.is_some());
    summaries.push(*summary);
    assert!(matches!(
        undo::peek_redo(Scope::Global).unwrap(),
        Status::Available { .. }
    ));
    let redo = undo::start_redo(Scope::Global).unwrap();
    let (outcome, more) = finish(rx, redo.handle.id);
    events.extend(more);
    assert_eq!(outcome, JobOutcome::Succeeded);
    assert!(matches!(
        redo.result.result().unwrap(),
        Some(undo::UndoResult::Global { .. })
    ));
    assert!(matches!(
        undo::peek_undo(Scope::Session { session_id: u64::MAX }),
        Err(CoreError::InvalidArgument { .. })
    ));
    let review_session::ReviewStartResult::Started { session_id, head, .. } = start_review(rx, events, zoo) else {
        panic!("review start")
    };
    let card = &head.items[0];
    assert!(matches!(
        review_session::answer_session(
            session_id,
            card.card_id,
            review_session::ReviewRating::Easy,
            card.state_token.clone(),
            NOW,
            2_000
        )
        .unwrap(),
        review_session::ReviewAnswerOutcome::Accepted { .. }
    ));
    let scoped = undo::start_undo(Scope::Session { session_id }).unwrap();
    let (outcome, more) = finish(rx, scoped.handle.id);
    events.extend(more);
    assert_eq!(outcome, JobOutcome::Succeeded);
    let Some(undo::UndoResult::Session { head }) = scoped.result.result().unwrap() else {
        panic!("session bypassed U43")
    };
    assert_eq!(head.items[0].card_id, card.card_id);
    let scoped_redo = undo::start_redo(Scope::Session { session_id }).unwrap();
    let (outcome, more) = finish(rx, scoped_redo.handle.id);
    events.extend(more);
    assert_eq!(outcome, JobOutcome::Succeeded);
    assert!(matches!(
        scoped_redo.result.result().unwrap(),
        Some(undo::UndoResult::Session { .. })
    ));
    // U32 must flush the actual session participant and retain its identity.
    suspend::flush_for_background().unwrap();
    assert_eq!(suspend::suspend(5_000).unwrap(), suspend::SuspendOutcome::Suspended);
    assert!(tags::mark_notes(vec![note.id]).is_err());
    let resumed = suspend::resume().unwrap();
    assert_eq!(resumed.active_session, Some(session_id));
    end_review(rx, events, session_id);
    let imported = notes::read_note(card.note_id).unwrap();
    notes::update_note(notes::UpdateNoteRequest {
        id: imported.id,
        fields: imported.fields,
        tags: vec!["imported-tag-edit".into()],
    })
    .unwrap();
    tags::mark_notes(vec![imported.id]).unwrap();
    tags::set_card_flag(vec![card.card_id], 3).unwrap();
    let note_before = notes::read_note(note.id).unwrap();
    let decks_before = deck_tree::deck_tree_with_counts().unwrap();
    let deletion_before = decks::deck_deletion_summary(vec![deck.id]).unwrap();
    assert_eq!(deletion_before.cards, 1);
    // Refuse deletion through the real safety provider's capacity preflight.
    low_space.store(true, Ordering::SeqCst);
    let deletion = decks::start_delete_decks(vec![deck.id]).unwrap();
    let (outcome, more) = finish(rx, deletion.handle.id);
    low_space.store(false, Ordering::SeqCst);
    events.extend(more);
    let JobOutcome::Failed {
        error: CoreError::Internal { message },
    } = outcome
    else {
        panic!("the real safety backup must fail before deletion")
    };
    assert!(message.contains("safety backup failed: "));
    assert!(message.contains("InsufficientSpace"));
    assert!(message.contains("available: 0"));
    assert!(message.contains("Unpublished"));
    assert!(deletion.result.result().unwrap().is_none());
    assert_eq!(decks::deck_deletion_summary(vec![deck.id]).unwrap(), deletion_before);
    assert_eq!(deck_tree::deck_tree_with_counts().unwrap(), decks_before);
    assert_eq!(notes::read_note(note.id).unwrap(), note_before);
    assert_eq!(notes::read_note(note.id).unwrap().guid, "surface-guid");
    assert_eq!(
        meguri_ffi::media::add_media("Surface.PNG".into(), vec![1, 2, 3]).unwrap(),
        "surface.png"
    );
    // Drain a final real job, then compare exact exported metadata to the
    // process dispatcher's commit envelopes (not a racing stream last_seq).
    let empty = decks::add_deck("Empty Surface".into()).unwrap();
    let deletion = decks::start_delete_decks(vec![empty.id]).unwrap();
    let (outcome, more) = finish(rx, deletion.handle.id);
    events.extend(more);
    assert_eq!(outcome, JobOutcome::Succeeded);
    assert_eq!(deletion.result.result().unwrap().unwrap().deleted.cards, 0);
    let addition = notes::start_add_notes(vec![notes::AddNoteRequest {
        notetype_id: nt,
        fields: vec!["delete me".into(), "back".into()],
        tags: vec![],
        deck_id: deck.id,
        guid: None,
        update_add_defaults: false,
    }])
    .unwrap();
    let (outcome, more) = finish(rx, addition.handle.id);
    events.extend(more);
    assert_eq!(outcome, JobOutcome::Succeeded);
    let added = addition.result.result().unwrap().unwrap();
    assert_eq!(added.notes.len(), 1);
    assert_eq!(added.summary.undo_name.as_deref(), Some("Add Note"));
    let transient = &added.notes[0];
    let remove = notes::start_delete_notes(vec![transient.id]).unwrap();
    let (outcome, more) = finish(rx, remove.handle.id);
    events.extend(more);
    assert_eq!(outcome, JobOutcome::Succeeded);
    let removed = remove.result.result().unwrap().unwrap();
    assert_eq!(removed.count, 1);
    assert!(removed.summary.is_some());
    let absent = notes::start_delete_notes(vec![transient.id]).unwrap();
    let (outcome, more) = finish(rx, absent.handle.id);
    events.extend(more);
    assert_eq!(outcome, JobOutcome::Succeeded);
    let absent = absent.result.result().unwrap().unwrap();
    assert_eq!(absent.count, 0);
    assert!(absent.summary.is_none());
    for summary in summaries {
        assert!(summary.changes.skipped_incoming_cards.is_empty());
        assert!(summary.changes.skipped_incoming_notes.is_empty());
        assert!(summary.changes.skipped_incoming_decks.is_empty());
        let envelopes: Vec<_> = events
            .iter()
            .filter(|e| e.seq == summary.seq && matches!(&e.kind, CoreEventKind::Changes { .. }))
            .collect();
        if changes_empty(&summary.changes) {
            assert!(
                envelopes.is_empty(),
                "no-op published Changes: {summary:?}; {envelopes:?}"
            );
            continue;
        }
        assert_eq!(
            envelopes.len(),
            1,
            "expected one commit envelope: {summary:?}; {envelopes:?}"
        );
        let event = envelopes[0];
        assert_eq!(event.generation, summary.generation, "summary: {summary:?}");
        assert_eq!(
            event.kind,
            CoreEventKind::Changes {
                changes: summary.changes.clone()
            },
            "summary: {summary:?}"
        );
    }
    (note.id, nt)
}
fn verify_imported_sentinels(location: &CollectionLocation, nt: i64) {
    let host = meguri_host::CoreHost::open(meguri_host::HostConfig::new(
        location.collection_directory.clone().into(),
        location.backups_directory.clone().into(),
        location.app_group_snapshot.clone().into(),
        Arc::new(FixedClock::new(NOW, UtcOffset::UTC)),
        Arc::new(SeededRandom::new(37)),
        Arc::new(StatvfsSpace),
    ))
    .unwrap();
    let c = host.collection().unwrap();
    {
        let r = c.reader(ReaderRole::Query).unwrap();
        let n = r.notetype(meguri_model::ids::NotetypeId(nt)).unwrap().unwrap();
        assert_eq!(n.config.latex_pre, "imported latex preamble");
        assert!(n.config.latex_svg);
        assert_eq!(n.config.css, ".card { color: green; }");
        for fields in [
            n.config.unknown_fields(),
            n.fields[0].config.unknown_fields(),
            n.templates[0].config.unknown_fields(),
        ] {
            assert!(fields.iter().any(|f| f.number() == 20 && f.bytes() == [0xa0, 0x01, 7]));
        }
        let imported = r
            .notes_after(None, 100)
            .unwrap()
            .into_iter()
            .filter(|n| n.data == "imported note data")
            .collect::<Vec<_>>();
        assert_eq!(imported.len(), 3);
        assert!(imported.iter().any(|n| n.tags.contains("imported-tag-edit")));
        for n in &imported {
            assert_eq!(n.flags, 64);
            for card in r.cards_of_note(n.id).unwrap() {
                assert_eq!(card.flags & !7, 0xa0);
            }
        }
    }
    assert!(host.media().unwrap().store().lookup("surface.png").unwrap().is_some());
    host.close().unwrap();
}

#[cfg(feature = "test-hooks")]
fn controlled_job_regressions(rx: &mpsc::Receiver<CoreEvent>, events: &mut Vec<CoreEvent>) {
    use meguri_engine::Collection;
    use meguri_ffi::{notes, review_session::ReviewUndoScope, undo};
    use meguri_model::ids::NoteId;
    struct Release(mpsc::Sender<()>);
    impl Drop for Release {
        fn drop(&mut self) {
            let _ = self.0.send(());
        }
    }
    fn block(c: &Collection) -> (Release, meguri_engine::Pending<meguri_engine::OpResult<()>>) {
        let (enter, entered) = mpsc::channel();
        let (release, wait) = mpsc::channel();
        let pending = c.submit(OpSpec::named("ffi cancellation gate", OpSource::Rollover), move |_| {
            enter.send(()).unwrap();
            wait.recv_timeout(Duration::from_secs(10)).unwrap();
            Ok(())
        });
        entered.recv_timeout(Duration::from_secs(5)).unwrap();
        (Release(release), pending)
    }
    fn queued(c: &Collection, name: &str) {
        let deadline = meguri_engine::monotonic::now() + Duration::from_secs(5);
        while !c.has_queued_op_for_testing(name) {
            assert!(meguri_engine::monotonic::now() < deadline, "never queued {name}");
            std::thread::yield_now();
        }
    }
    fn dump(c: &Collection) -> Vec<String> {
        let r = c.reader(ReaderRole::Query).unwrap();
        ["notes", "cards", "graves", "col", "decks"]
            .iter()
            .map(|table| {
                format!(
                    "{:?}",
                    r.execute_raw(&format!("SELECT * FROM {table} ORDER BY 1"), &[])
                        .unwrap()
                        .rows
                )
            })
            .collect()
    }
    fn stacks(c: &Collection) -> (meguri_engine::UndoStatus, meguri_engine::UndoStatus) {
        (
            c.peek_undo(meguri_engine::UndoScope::Global).unwrap(),
            c.peek_redo(meguri_engine::UndoScope::Global).unwrap(),
        )
    }
    // Execute each short export on a registered caller, with the writer
    // positively occupied. A blocking export cannot pass on a later release.
    fn prompt<T: Send + 'static>(call: impl FnOnce() -> T + Send + 'static) -> T {
        let threads = meguri_engine::ThreadRegistry::default();
        let (send, recv) = mpsc::channel();
        let caller = threads
            .spawn("ffi-lifecycle-caller", meguri_engine::Qos::Default, move |_| {
                let _ = send.send(call());
            })
            .unwrap();
        let value = recv
            .recv_timeout(Duration::from_secs(1))
            .expect("short lifecycle export blocked");
        caller.join().unwrap();
        value
    }
    let c = meguri_ffi::test_hooks::collection_for_testing().unwrap();
    let deck = deck_tree::deck_tree_with_counts().unwrap().nodes[0].id;
    let (release, blocked) = block(&c);
    let start = prompt(move || review_session::start_session(deck, None)).unwrap();
    queued(&c, "admit_review_lifecycle");
    assert!(start.result.result().unwrap().is_none());
    // No session has been installed, but the queued start already owns the
    // lifecycle. A refused close must also restore the runtime reservation.
    for _ in 0..2 {
        assert_eq!(
            prompt(lifecycle::close_collection),
            Err(CoreError::Busy {
                running_op: "review lifecycle".into()
            })
        );
        assert_eq!(
            meguri_ffi::test_hooks::collection_for_testing().unwrap().generation(),
            c.generation()
        );
        assert_eq!(deck_tree::deck_tree_with_counts().unwrap().generation, c.generation().0);
    }
    start.handle.cancel.cancel().unwrap();
    drop(release);
    blocked.wait().unwrap();
    let (outcome, more) = finish(rx, start.handle.id);
    events.extend(more);
    assert_eq!(outcome, JobOutcome::Cancelled);
    assert!(start.result.result().unwrap().is_none());

    let review_session::ReviewStartResult::Started {
        session_id: id, head, ..
    } = start_review(rx, events, deck)
    else {
        panic!("lifecycle fixture must start")
    };
    for restart in [true, false] {
        let (release, blocked) = block(&c);
        let start = if restart {
            Some(prompt(move || review_session::start_session(deck, None)).unwrap())
        } else {
            None
        };
        let end = if !restart {
            Some(prompt(move || review_session::end_session(id)).unwrap())
        } else {
            None
        };
        let handle = start.as_ref().map(|j| &j.handle).or(end.as_ref()).unwrap();
        queued(&c, "admit_review_lifecycle");
        assert_eq!(review_session::session_head(id, 3).unwrap(), head);
        handle.cancel.cancel().unwrap();
        drop(release);
        blocked.wait().unwrap();
        let (outcome, more) = finish(rx, handle.id);
        events.extend(more);
        assert_eq!(outcome, JobOutcome::Cancelled);
        if let Some(start) = start {
            assert!(start.result.result().unwrap().is_none());
        }
        assert_eq!(review_session::session_head(id, 3).unwrap(), head);
    }

    // A real restore owns the logical lifecycle while the writer is parked.
    // Both exported jobs must complete with the actual busy error before it
    // is released; internal synchronous start/end serialization is retained
    // in session_controlled's existing concurrency test.
    let added = meguri_engine::decks::add(&c, "Lifecycle restoration".into())
        .unwrap()
        .value
        .id;
    let (release, blocked) = block(&c);
    assert!(!c.has_queued_op_for_testing("durability_control"));
    let restore = undo::start_undo(ReviewUndoScope::Global).unwrap();
    queued(&c, "durability_control");
    // Exercise the real runtime Closing reservation twice: refusal must retain
    // this usable host and session, rather than marking either disconnected.
    for _ in 0..2 {
        assert_eq!(
            prompt(lifecycle::close_collection),
            Err(CoreError::Busy {
                running_op: "review lifecycle".into()
            })
        );
        let retained = meguri_ffi::test_hooks::collection_for_testing().unwrap();
        assert_eq!(retained.generation(), c.generation());
        assert_eq!(deck_tree::deck_tree_with_counts().unwrap().generation, c.generation().0);
        assert_eq!(review_session::session_head(id, 3).unwrap(), head);
    }
    let refused = prompt(move || review_session::start_session(deck, None)).unwrap();
    let (outcome, more) = finish(rx, refused.handle.id);
    events.extend(more);
    assert_eq!(
        outcome,
        JobOutcome::Failed {
            error: CoreError::Unavailable {
                reason: "review lifecycle is busy".into()
            }
        }
    );
    assert!(refused.result.result().unwrap().is_none());
    let refused = prompt(move || review_session::end_session(id)).unwrap();
    let (outcome, more) = finish(rx, refused.id);
    events.extend(more);
    assert_eq!(
        outcome,
        JobOutcome::Failed {
            error: CoreError::Unavailable {
                reason: "review lifecycle is busy".into()
            }
        }
    );
    assert!(restore.result.result().unwrap().is_none());
    assert_eq!(review_session::session_head(id, 3).unwrap(), head);
    drop(release);
    blocked.wait().unwrap();
    let (outcome, more) = finish(rx, restore.handle.id);
    events.extend(more);
    assert_eq!(outcome, JobOutcome::Succeeded);
    assert!(matches!(
        restore.result.result().unwrap(),
        Some(undo::UndoResult::Session { .. })
    ));
    assert!(c.reader(ReaderRole::Query).unwrap().deck(added).unwrap().is_none());

    let (release, blocked) = block(&c);
    let restart = prompt(move || review_session::start_session(deck, None)).unwrap();
    queued(&c, "admit_review_lifecycle");
    assert!(restart.result.result().unwrap().is_none());
    assert!(review_session::session_head(id, 3).is_ok());
    let (enter, entered) = mpsc::channel();
    let (commit, wait) = mpsc::channel();
    let commit_release = Release(commit);
    restart
        .handle
        .progress
        .set_listener(Arc::new(FfiCommitGate {
            enter,
            release: Mutex::new(wait),
        }))
        .unwrap();
    drop(release);
    blocked.wait().unwrap();
    entered.recv_timeout(Duration::from_secs(5)).unwrap();
    // Cancellation after admission must retain the completed replacement.
    restart.handle.cancel.cancel().unwrap();
    drop(commit_release);
    let (outcome, more) = finish(rx, restart.handle.id);
    events.extend(more);
    assert_eq!(outcome, JobOutcome::Succeeded);
    let Some(review_session::ReviewStartResult::Started {
        session_id: next, head, ..
    }) = restart.result.result().unwrap()
    else {
        panic!("restart must really complete")
    };
    assert_ne!(next, id);
    assert!(review_session::session_head(id, 3).is_err());
    let (release, blocked) = block(&c);
    let card = &head.items[0];
    assert!(matches!(
        review_session::answer_session(
            next,
            card.card_id,
            review_session::ReviewRating::Easy,
            card.state_token.clone(),
            NOW,
            100
        )
        .unwrap(),
        review_session::ReviewAnswerOutcome::Accepted { .. }
    ));
    let end = prompt(move || review_session::end_session(next)).unwrap();
    queued(&c, "admit_review_lifecycle");
    assert!(review_session::session_head(next, 3).is_ok());
    let (enter, entered) = mpsc::channel();
    let (commit, wait) = mpsc::channel();
    let commit_release = Release(commit);
    end.progress
        .set_listener(Arc::new(FfiCommitGate {
            enter,
            release: Mutex::new(wait),
        }))
        .unwrap();
    drop(release);
    blocked.wait().unwrap();
    entered.recv_timeout(Duration::from_secs(5)).unwrap();
    end.cancel.cancel().unwrap();
    drop(commit_release);
    let (outcome, more) = finish(rx, end.id);
    assert!(more.iter().any(|e| matches!(
        e.kind, CoreEventKind::SessionEnded { session_id } if session_id == next
    )));
    events.extend(more);
    assert_eq!(outcome, JobOutcome::Succeeded);
    assert!(review_session::session_head(next, 3).is_err());
    let failed = review_session::end_session(next).unwrap();
    let (outcome, more) = finish(rx, failed.id);
    events.extend(more);
    assert_eq!(
        outcome,
        JobOutcome::Failed {
            error: CoreError::InvalidArgument {
                message: "NoSession".into()
            }
        }
    );
    // Restore the accepted answer through the real global job so subsequent
    // fixture assertions still see the original two fresh cards.
    let restore_answer = undo::start_undo(ReviewUndoScope::Global).unwrap();
    let (outcome, more) = finish(rx, restore_answer.handle.id);
    events.extend(more);
    assert_eq!(outcome, JobOutcome::Succeeded);
    assert!(matches!(
        restore_answer.result.result().unwrap(),
        Some(undo::UndoResult::Global { .. })
    ));

    let nid = c
        .reader(ReaderRole::Query)
        .unwrap()
        .ids_after(meguri_storage::Table::Notes, None, 1)
        .unwrap()[0];
    let before = dump(&c);
    let history = stacks(&c);
    let log = c.path().with_file_name("deleted.txt");
    let deleted = std::fs::read(&log).ok();
    let (release, blocked) = block(&c);
    let deletion = notes::start_delete_notes(vec![nid]).unwrap();
    queued(&c, meguri_engine::undo::names::DELETE_NOTE);
    deletion.handle.cancel.cancel().unwrap();
    drop(release);
    blocked.wait().unwrap();
    let (outcome, more) = finish(rx, deletion.handle.id);
    events.extend(more);
    assert_eq!(outcome, JobOutcome::Cancelled);
    assert!(deletion.result.result().unwrap().is_none());
    assert_eq!(dump(&c), before);
    assert_eq!(stacks(&c), history);
    assert_eq!(std::fs::read(&log).ok(), deleted);

    meguri_engine::notes::delete(&c, vec![NoteId(nid); 501], OpSource::UserUndoable)
        .unwrap()
        .unwrap();
    for redo in [false, true] {
        if redo {
            let job = undo::start_undo(ReviewUndoScope::Global).unwrap();
            let (outcome, more) = finish(rx, job.handle.id);
            events.extend(more);
            assert_eq!(outcome, JobOutcome::Succeeded);
        }
        let before = dump(&c);
        let history = stacks(&c);
        let (release, blocked) = block(&c);
        let job = if redo {
            undo::start_redo(ReviewUndoScope::Global)
        } else {
            undo::start_undo(ReviewUndoScope::Global)
        }
        .unwrap();
        queued(&c, if redo { "redo" } else { "undo" });
        job.handle.cancel.cancel().unwrap();
        drop(release);
        blocked.wait().unwrap();
        let (outcome, more) = finish(rx, job.handle.id);
        events.extend(more);
        assert_eq!(outcome, JobOutcome::Cancelled);
        assert!(job.result.result().unwrap().is_none());
        assert_eq!(dump(&c), before);
        assert_eq!(stacks(&c), history);
    }
    let redo = undo::start_redo(ReviewUndoScope::Global).unwrap();
    let (outcome, more) = finish(rx, redo.handle.id);
    events.extend(more);
    assert_eq!(outcome, JobOutcome::Succeeded);
    let restore = undo::start_undo(ReviewUndoScope::Global).unwrap();
    let (outcome, more) = finish(rx, restore.handle.id);
    events.extend(more);
    assert_eq!(outcome, JobOutcome::Succeeded);

    struct FfiCommitGate {
        enter: mpsc::Sender<()>,
        release: Mutex<mpsc::Receiver<()>>,
    }
    impl meguri_ffi::ProgressListener for FfiCommitGate {
        fn on_progress(&self, p: meguri_ffi::ProgressUpdate) {
            if p.phase == meguri_ffi::progress::JobProgressPhase::Committing {
                self.enter.send(()).unwrap();
                self.release
                    .lock()
                    .unwrap()
                    .recv_timeout(Duration::from_secs(10))
                    .unwrap();
            }
        }
    }
    for restoring in [false, true] {
        let (release, blocked) = block(&c);
        let deletion = if !restoring {
            Some(notes::start_delete_notes(vec![nid; 501]).unwrap())
        } else {
            None
        };
        let restoration = if restoring {
            Some(undo::start_undo(ReviewUndoScope::Global).unwrap())
        } else {
            None
        };
        let handle = if let Some(job) = &deletion {
            &job.handle
        } else {
            &restoration.as_ref().unwrap().handle
        };
        queued(&c, if restoring { "undo" } else { "bulk_op" });
        let (enter, entered) = mpsc::channel();
        let (finish_commit, wait) = mpsc::channel();
        let _commit_release = Release(finish_commit.clone());
        handle
            .progress
            .set_listener(Arc::new(FfiCommitGate {
                enter,
                release: Mutex::new(wait),
            }))
            .unwrap();
        drop(release);
        blocked.wait().unwrap();
        entered.recv_timeout(Duration::from_secs(5)).unwrap();
        handle.cancel.cancel().unwrap();
        let _ = finish_commit.send(());
        let (outcome, more) = finish(rx, handle.id);
        events.extend(more);
        assert_eq!(outcome, JobOutcome::Succeeded);
        if let Some(job) = deletion {
            assert!(job.result.result().unwrap().unwrap().summary.unwrap().past_cancel_point);
        }
        if let Some(job) = restoration {
            let Some(undo::UndoResult::Global { summary, step }) = job.result.result().unwrap() else {
                panic!("global restore result")
            };
            assert!(summary.past_cancel_point);
            assert!(step.long_write.is_some());
        }
    }
}
