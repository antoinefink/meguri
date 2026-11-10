use super::{NOW, generated};
use meguri_engine::{Collection, media::MediaService};
use meguri_model::{
    clock::{Clock, FixedClock, UtcOffset},
    rng::SeededRandom,
};
use meguri_storage::{
    media::{MediaFeedSnapshot, MediaFeedStatus},
    rusqlite::{Connection, OpenFlags},
};
use meguri_sync::cloud::{
    BindingId, BuildScope, OpaqueBytes, TRACKING_FILE,
    adapter::{Guard, Snapshot},
    row_payload::KINDS,
    runtime::{
        CloudInputs, IdentityProvider, IdentityStatus, Runtime, RuntimeError, StoredRuntimeState, with_tracking_access,
    },
    workflow::{EntityFacts, Observation, PendingChange},
};
use meguri_testkit::cloud::{
    Config, Service,
    devices::{NativeFiles, PreboundDevice, options},
};
use serde_json::Value;
use std::{
    collections::{BTreeMap, VecDeque},
    panic::{AssertUnwindSafe, catch_unwind, resume_unwind},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc::{self, Receiver, Sender},
    },
    thread::JoinHandle,
    time::Duration,
};

const BOUND: Duration = Duration::from_secs(10);

struct Account;
impl IdentityProvider for Account {
    fn read_identity(&self, _: &BuildScope) -> IdentityStatus {
        IdentityStatus::Available(OpaqueBytes(b"synthetic-account-a".to_vec()))
    }
}

struct Gate {
    entered: Sender<()>,
    release: Receiver<()>,
}
struct ArmedClock {
    gates: Mutex<VecDeque<Gate>>,
    calls: AtomicUsize,
    timed_out: AtomicBool,
}
impl std::fmt::Debug for ArmedClock {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ArmedClock")
    }
}
impl Clock for ArmedClock {
    fn now_ms(&self) -> i64 {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let gate = { self.gates.lock().unwrap().pop_front() };
        if let Some(gate) = gate
            && (gate.entered.send(()).is_err() || gate.release.recv_timeout(BOUND).is_err())
        {
            self.timed_out.store(true, Ordering::SeqCst);
        }
        NOW
    }
    fn utc_offset_at(&self, _: i64) -> UtcOffset {
        UtcOffset::UTC
    }
    fn zone_id(&self) -> String {
        "UTC".into()
    }
}

#[derive(Clone, PartialEq)]
struct Observed {
    ledger: Value,
    stored: StoredRuntimeState,
    entities: Vec<EntityFacts>,
    adapter: Snapshot,
    guard: Guard,
    feed: MediaFeedSnapshot,
    status: MediaFeedStatus,
    pending: BTreeMap<String, PendingChange>,
    emitted: Vec<u64>,
    observations: Vec<Observation>,
}

fn observe(runtime: &Runtime, media: &MediaService, readonly: &Connection, binding: BindingId) -> Observed {
    // Read the already-admitted original sidecar, never a private WAL copy or
    // a new TrackingStore open. Decode the entire private ledger in this test.
    let ledger = with_tracking_access(|| {
        let text: String = readonly
            .query_row(
                "SELECT content FROM cloud_media_state WHERE binding=?1",
                [binding.0.as_slice()],
                |row| row.get(0),
            )
            .unwrap();
        serde_json::from_str(&text).unwrap()
    });
    let mut entities = Vec::new();
    for kind in KINDS {
        let mut after = None;
        for _ in 0..32 {
            let page = runtime.entity_facts_page(kind, after.as_deref()).unwrap();
            if page.is_empty() {
                break;
            }
            after = page.last().map(|entity| entity.key.clone());
            entities.extend(page);
            assert!(entities.len() <= 2048, "bounded synthetic facts");
        }
        assert!(runtime.entity_facts_page(kind, after.as_deref()).unwrap().is_empty());
    }
    Observed {
        ledger,
        stored: runtime.stored_state_for_testing().unwrap(),
        entities,
        adapter: runtime.adapter_snapshot().unwrap(),
        guard: runtime.adapter_guard(),
        feed: media.store().media_feed_snapshot().unwrap(),
        status: media.store().media_feed_status().unwrap(),
        pending: runtime.tracking_sendables(),
        emitted: runtime.emitted_update_ids(),
        observations: runtime.observations(),
    }
}

fn spawn_consumer(
    runtime: Arc<Runtime>,
    collection: Collection,
    media: MediaService,
    done: Sender<()>,
) -> JoinHandle<Result<usize, RuntimeError>> {
    std::thread::spawn(move || {
        let result = runtime.consume_media_feed(&collection, &media);
        let _ = done.send(());
        result
    })
}

#[test]
fn two_real_empty_feed_consumers_coalesce_in_first_started_order() {
    consume_pair(0);
}

#[test]
fn two_real_empty_feed_consumers_coalesce_in_second_started_order() {
    consume_pair(1);
}

fn consume_pair(first: usize) {
    let run = tempfile::tempdir().unwrap();
    let directory = generated(run.path(), "identical-feed");
    let mut service = Service::open(run.path(), Config::default(), SeededRandom::new(1919)).unwrap();
    let files = PreboundDevice::arrange(&directory, &mut service, 1919).unwrap();
    let fixed = Arc::new(FixedClock::new(NOW, UtcOffset::UTC));
    let mut config = options(&directory, NOW, 1919);
    config.clock = fixed.clone();
    config.cloud = Some(CloudInputs {
        scope: files.binding.scope.clone(),
        identity: Arc::new(Account),
        files: Arc::new(NativeFiles),
    });
    let host = meguri_host::CoreHost::open(config).unwrap();
    let runtime = host.cloud_runtime().unwrap().clone();
    let collection = host.collection().unwrap();
    let media = host.media().unwrap();
    media.store().register_full(|_, _| true).unwrap();
    let feed = media.store().media_feed_snapshot().unwrap();
    assert!(feed.status.complete && feed.items.is_empty());
    runtime.refresh_identity().unwrap();
    runtime
        .reconcile_binding(&collection, &media, Duration::from_secs(30))
        .unwrap();
    assert_eq!(runtime.consume_media_feed(&collection, &media).unwrap(), 0);
    let admitted = runtime.status().unwrap();
    assert_eq!(admitted.binding.as_ref().unwrap().id, files.binding.id);
    assert!(admitted.stored_usable && admitted.identity_verified);
    // Local reconciliation completes this open's scans, not the durable drain.
    assert!(admitted.scan_completed_this_open && admitted.note_scan_completed_this_open);
    assert!(!admitted.scan_drained);
    assert!(!admitted.scan_required && !admitted.tracking_stopped);
    let readonly = with_tracking_access(|| {
        Connection::open_with_flags(directory.join(TRACKING_FILE), OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap()
    });
    let baseline = observe(&runtime, &media, &readonly, files.binding.id);
    assert!(!baseline.entities.is_empty());
    assert!(baseline.feed.items.is_empty() && baseline.status.complete);
    assert_eq!(baseline.status, baseline.feed.status);
    assert_eq!(baseline.status.floor, Some(baseline.status.counter));
    assert_eq!(
        baseline.ledger["feed"],
        serde_json::json!([baseline.status.feed_id.0, baseline.status.counter])
    );
    let mut gates = VecDeque::new();
    let mut entered = Vec::new();
    let mut releases = Vec::new();
    let mut done_senders = Vec::new();
    let mut done = Vec::new();
    for _ in 0..2 {
        let (entered_tx, entered_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let (done_tx, done_rx) = mpsc::channel();
        gates.push_back(Gate {
            entered: entered_tx,
            release: release_rx,
        });
        entered.push(entered_rx);
        releases.push(release_tx);
        done_senders.push(done_tx);
        done.push(done_rx);
    }
    let clock = Arc::new(ArmedClock {
        gates: Mutex::new(gates),
        calls: AtomicUsize::new(0),
        timed_out: AtomicBool::new(false),
    });
    // Only Runtime gets gates. Host, collection and MediaService keep fixed time.
    // With an empty feed, the first Runtime Clock call is terminal media_now:
    // after the captured ledger/proposal and before final State/tracking access.
    runtime.attach_adapter_clock(clock.clone());
    let mut handles = [None, None];
    let outcome = catch_unwind(AssertUnwindSafe(|| {
        handles[0] = Some(spawn_consumer(
            runtime.clone(),
            collection.clone(),
            media.clone(),
            done_senders[0].clone(),
        ));
        entered[0]
            .recv_timeout(BOUND)
            .expect("consumer one reached terminal clock");
        handles[1] = Some(spawn_consumer(
            runtime.clone(),
            collection.clone(),
            media.clone(),
            done_senders[1].clone(),
        ));
        entered[1]
            .recv_timeout(BOUND)
            .expect("consumer two captured the same ledger");
        assert_eq!(clock.calls.load(Ordering::SeqCst), 2);
        assert!(clock.gates.lock().unwrap().is_empty());
        releases[first].send(()).unwrap();
        done[first].recv_timeout(BOUND).expect("first completion");
        let result = handles[first].take().unwrap().join().unwrap();
        assert!(matches!(result, Ok(0)), "first consumer: {result:?}");
        let post_first = observe(&runtime, &media, &readonly, files.binding.id);
        assert_eq!(
            clock.calls.load(Ordering::SeqCst),
            2,
            "readback must not consume clocks"
        );
        assert!(
            post_first == baseline,
            "an already current feed must preserve all persisted state"
        );
        let second = 1 - first;
        releases[second].send(()).unwrap();
        done[second].recv_timeout(BOUND).expect("second completion");
        let result = handles[second].take().unwrap().join().unwrap();
        // Keep this positive assertion identical for the before/after-fix runs.
        assert!(matches!(result, Ok(0)), "second consumer: {result:?}");
        let post_second = observe(&runtime, &media, &readonly, files.binding.id);
        assert!(
            post_second == post_first,
            "redundant consumer must change nothing, including revision"
        );
        assert_eq!(clock.calls.load(Ordering::SeqCst), 2);
    }));
    // Release every gate before joining any remaining original handle, even
    // when an entered/result/readback assertion failed. Never wait under locks.
    for release in releases {
        let _ = release.send(());
    }
    let mut joined = true;
    for handle in handles.into_iter().flatten() {
        joined &= handle.join().is_ok();
    }
    runtime.attach_adapter_clock(fixed);
    drop(readonly);
    host.close().unwrap();
    if let Err(panic) = outcome {
        resume_unwind(panic);
    }
    assert!(joined, "every original consumer joined successfully");
    assert!(
        !clock.timed_out.load(Ordering::SeqCst),
        "clock gates retained their ten-second bounds"
    );
}
