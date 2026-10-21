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
fn location(root: &Path) -> CollectionLocation {
    let collection = root.join("collection");
    let backups = root.join("backups");
    std::fs::create_dir_all(&collection).unwrap();
    std::fs::create_dir_all(&backups).unwrap();
    CollectionLocation {
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
    let open = || collection::start_open(location.clone(), Arc::new(Clock), Arc::new(Space), 37).unwrap();
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
    let review_session::ReviewStartResult::Started {
        session_id,
        generation,
        head,
        ..
    } = review_session::start_session(tree.nodes[0].id, None).unwrap()
    else {
        panic!("review must use the opened host")
    };
    assert_eq!(generation, 1);
    assert_eq!(head.counts.new, 2);
    assert!(!head.items.is_empty());
    review_session::end_session(session_id).unwrap();
    lifecycle::close_collection().unwrap();
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
    assert!(matches!(outcome, JobOutcome::Failed { .. }));
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
    assert!(
        events.windows(2).all(|e| e[0].seq < e[1].seq),
        "one process dispatcher preserves order across reopen"
    );
}
