//! The derived host's items added for U289 (`stats/history-reads` H14.3,
//! H18): a component's own rebuild request through its handle, and
//! `has_failed` for a component left `Stale` with no refresh to come.

mod common;

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use common::{copy_fixture, open_config, NOW_MS};
use meguri_engine::derived_host::{
    BuildSink, Capture, ComponentHandle, ComponentSchema, ComponentTables, DerivedComponent, DerivedHost,
    DerivedHostBuilder, Invalidation, Readiness, TableDef,
};
use meguri_engine::{Collection, Engine, EngineConfig, EngineError, ObservedCommit, Ports, Snapshot};
use meguri_model::clock::{StepClock, UtcOffset};

const WAIT: Duration = Duration::from_secs(60);
const NAME: &str = "rebuilt";

#[derive(Default)]
struct Seen {
    builds: AtomicUsize,
    /// Every build fails while set.
    failing: AtomicBool,
    handle: Mutex<Option<ComponentHandle>>,
}

/// A component with one empty table whose builds count, or fail.
struct Counted {
    deferred: Option<&'static str>,
    seen: Arc<Seen>,
}

impl DerivedComponent for Counted {
    type Delta = ();

    fn schema(&self) -> ComponentSchema {
        ComponentSchema::new(NAME, 1).table(TableDef::columns("rows", "(id INTEGER PRIMARY KEY)"))
    }

    fn capture(&self, _commit: &ObservedCommit<'_>) -> Capture<()> {
        Capture::Nothing
    }

    fn apply(&self, _tables: &ComponentTables<'_>, _delta: ()) -> Result<(), EngineError> {
        Ok(())
    }

    fn build(&self, _source: &Snapshot<'_>, _sink: &mut BuildSink) -> Result<(), EngineError> {
        self.seen.builds.fetch_add(1, Ordering::SeqCst);
        if self.seen.failing.load(Ordering::SeqCst) {
            return Err(EngineError::Internal {
                message: "this build is refused".into(),
            });
        }
        Ok(())
    }

    fn deferred_until(&self) -> Option<&'static str> {
        self.deferred
    }

    fn attach(&self, handle: ComponentHandle) {
        *self.seen.handle.lock().unwrap() = Some(handle);
    }
}

struct Opened {
    engine: Engine,
    _collection: Collection,
    host: DerivedHost,
    seen: Arc<Seen>,
}

impl Opened {
    fn readiness(&self) -> Readiness {
        self.host.readiness(NAME).expect("registered").readiness
    }

    fn builds(&self) -> usize {
        self.seen.builds.load(Ordering::SeqCst)
    }

    fn rebuild(&self) {
        self.seen.handle.lock().unwrap().as_ref().expect("attached").rebuild();
    }

    fn wait_ready(&self) {
        let deadline = meguri_engine::monotonic::now() + WAIT;
        while self.readiness() != Readiness::Ready {
            assert!(meguri_engine::monotonic::now() < deadline, "never became ready");
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    fn close(self) {
        self.engine.close().unwrap();
        self.host.close().unwrap();
    }
}

fn open(path: &std::path::Path, deferred: Option<&'static str>, failing: bool) -> Opened {
    let seen = Arc::new(Seen::default());
    seen.failing.store(failing, Ordering::SeqCst);
    let clock = Arc::new(StepClock::new(NOW_MS, UtcOffset::UTC, "UTC"));
    let engine = Engine::new(EngineConfig::default()).unwrap();
    let register = |builder: DerivedHostBuilder| {
        builder.component(Counted {
            deferred,
            seen: Arc::clone(&seen),
        })
    };
    let host = register(DerivedHost::builder(&engine)).start().unwrap();
    let ports = host.ports(Ports::default());
    let collection = engine.open_raw(path, open_config(&clock).ports(ports)).unwrap();
    Opened {
        engine,
        _collection: collection,
        host,
        seen,
    }
}

#[test]
fn h14_3_a_ready_component_asks_for_its_rebuild_and_a_held_one_is_unchanged() {
    let (_dir, path) = copy_fixture("schema-18.anki2");
    let opened = open(&path, Some("someSignal"), false);
    assert!(opened.host.wait_idle(WAIT));
    // Held at bind, not ready: the request changes nothing.
    opened.rebuild();
    assert!(opened.host.wait_idle(WAIT));
    assert_eq!(opened.readiness(), Readiness::Stale);
    assert_eq!(opened.builds(), 0);
    opened.host.request_refresh(NAME);
    opened.wait_ready();
    assert_eq!(opened.builds(), 1);
    // Ready: rebuilt once, through staging, and ready again.
    opened.rebuild();
    assert!(opened.host.wait_idle(WAIT));
    opened.wait_ready();
    assert_eq!(opened.builds(), 2);
    assert!(!opened.host.has_failed(NAME));
    opened.close();
}

#[test]
fn h18_a_component_whose_refresh_fails_twice_has_failed_until_an_invalidation() {
    let (_dir, path) = copy_fixture("schema-18.anki2");
    let opened = open(&path, None, true);
    let deadline = meguri_engine::monotonic::now() + WAIT;
    while !opened.host.has_failed(NAME) {
        assert!(meguri_engine::monotonic::now() < deadline, "the failures were never recorded");
        std::thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(opened.readiness(), Readiness::Stale);
    assert_eq!(opened.builds(), 2, "one retry, then no refresh to come");
    // Its own rebuild request leaves a failed component as it is.
    opened.rebuild();
    assert!(opened.host.wait_idle(WAIT));
    assert!(opened.host.has_failed(NAME));
    assert_eq!(opened.builds(), 2);
    // An invalidation queues a refresh again.
    opened.seen.failing.store(false, Ordering::SeqCst);
    opened.host.invalidate(Invalidation::CheckDatabase);
    opened.wait_ready();
    assert!(!opened.host.has_failed(NAME));
    opened.close();
}
