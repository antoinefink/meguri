//! The derived store's items added for U98 (`specs/storage/derived-store.md`
//! D4, D11, D13): a component's `deferred until` hold and the host's
//! `signal` and `request_refresh`; its `persist` at suspend and close and
//! through `release`, and its `release`.

mod common;

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use common::{NOW_MS, copy_fixture, note, open_config};
use meguri_engine::derived_host::{
    BuildSink, Capture, ComponentSchema, ComponentStore, ComponentTables, DerivedComponent, DerivedHost,
    DerivedHostBuilder, Readiness, TableDef,
};
use meguri_engine::{Collection, Engine, EngineConfig, EngineError, ObservedCommit, OpSpec, Ports, Snapshot};
use meguri_model::card::{Card, CardQueue, CardType};
use meguri_model::changes::{ChangeKind, OpSource};
use meguri_model::clock::{StepClock, UtcOffset};
use meguri_model::ids::{CardId, DeckId};
use meguri_model::time::EpochSecs;
use meguri_model::usn::Usn;
use meguri_storage::raw::SqlValue;
use meguri_storage::rusqlite;

const WAIT: Duration = Duration::from_secs(60);
const KEPT: &str = "kept";

/// What a test sees of a [`Keeper`].
#[derive(Default)]
struct Seen {
    builds: AtomicUsize,
    persists: AtomicUsize,
    releases: AtomicUsize,
    /// The next persist fails.
    fail_persist: AtomicBool,
    /// Added card ids applied and not yet persisted: the in-memory state.
    pending: Mutex<Vec<i64>>,
}

/// A component whose content is in memory until its persist writes it:
/// one row per added card. Its build writes every card; its applies only
/// keep the ids in memory (D4's overlay).
struct Keeper {
    name: &'static str,
    deferred: Option<&'static str>,
    seen: Arc<Seen>,
}

fn sql(error: rusqlite::Error) -> EngineError {
    EngineError::Internal {
        message: error.to_string(),
    }
}

fn put_ids(tables: &ComponentTables<'_>, ids: &[i64]) -> Result<(), EngineError> {
    let table = tables.table("rows")?;
    for id in ids {
        tables
            .conn()
            .execute(&format!("INSERT OR REPLACE INTO \"{table}\" (cid) VALUES (?1)"), [id])
            .map_err(sql)?;
    }
    Ok(())
}

impl DerivedComponent for Keeper {
    type Delta = Vec<i64>;

    fn schema(&self) -> ComponentSchema {
        ComponentSchema::new(self.name, 1).table(TableDef::columns("rows", "(cid INTEGER PRIMARY KEY)"))
    }

    fn capture(&self, commit: &ObservedCommit<'_>) -> Capture<Vec<i64>> {
        let added: Vec<i64> = commit
            .changes
            .cards
            .iter()
            .filter(|c| c.kind == ChangeKind::Added)
            .map(|c| c.id.0)
            .collect();
        if added.is_empty() {
            Capture::Nothing
        } else {
            Capture::Delta(added)
        }
    }

    fn apply(&self, _tables: &ComponentTables<'_>, delta: Vec<i64>) -> Result<(), EngineError> {
        self.seen.pending.lock().unwrap().extend(delta);
        Ok(())
    }

    fn build(&self, source: &Snapshot<'_>, sink: &mut BuildSink) -> Result<(), EngineError> {
        self.seen.builds.fetch_add(1, Ordering::SeqCst);
        let ids: Vec<i64> = source
            .execute_raw("SELECT id FROM cards", &[])?
            .rows
            .into_iter()
            .map(|row| match row[0] {
                SqlValue::Integer(id) => id,
                ref other => panic!("{other:?}"),
            })
            .collect();
        sink.write(move |tables| put_ids(tables, &ids))
    }

    fn deferred_until(&self) -> Option<&'static str> {
        self.deferred
    }

    fn persist(&self, store: &mut ComponentStore<'_>) -> Result<(), EngineError> {
        self.seen.persists.fetch_add(1, Ordering::SeqCst);
        if self.seen.fail_persist.swap(false, Ordering::SeqCst) {
            return Err(EngineError::Internal {
                message: "this persist is refused".into(),
            });
        }
        let pending = std::mem::take(&mut *self.seen.pending.lock().unwrap());
        store.write(move |tables| put_ids(tables, &pending))
    }

    fn release(&self) {
        self.seen.releases.fetch_add(1, Ordering::SeqCst);
        self.seen.pending.lock().unwrap().clear();
    }
}

struct Opened {
    engine: Engine,
    collection: Collection,
    host: DerivedHost,
    ports: Ports,
}

impl Opened {
    fn readiness(&self, name: &str) -> Readiness {
        self.host.readiness(name).expect("registered").readiness
    }

    fn close(self) {
        self.engine.close().unwrap();
        self.host.close().unwrap();
    }

    fn wait_ready(&self, name: &str) {
        let deadline = Instant::now() + WAIT;
        while self.readiness(name) != Readiness::Ready {
            assert!(Instant::now() < deadline, "{name} never became ready");
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    /// The rows the store's file holds for `name`, as a reader sees them.
    fn stored(&self, name: &str) -> Vec<i64> {
        let lease = self.collection.reader(meguri_storage::ReaderRole::Query).unwrap();
        let snapshot = lease.snapshot().unwrap();
        let mut ids: Vec<i64> = snapshot
            .execute_raw(&format!("SELECT cid FROM derived.\"{name}__rows\""), &[])
            .unwrap()
            .rows
            .into_iter()
            .map(|row| match row[0] {
                SqlValue::Integer(id) => id,
                ref other => panic!("{other:?}"),
            })
            .collect();
        ids.sort_unstable();
        ids
    }
}

fn open(path: &std::path::Path, register: impl FnOnce(DerivedHostBuilder) -> DerivedHostBuilder) -> Opened {
    let clock = Arc::new(StepClock::new(NOW_MS, UtcOffset::UTC, "UTC"));
    let engine = Engine::new(EngineConfig::default()).unwrap();
    let host = register(DerivedHost::builder(&engine)).start().unwrap();
    let ports = host.ports(Ports::default());
    let collection = engine
        .open_raw(path, open_config(&clock).ports(ports.clone()))
        .unwrap();
    Opened {
        engine,
        collection,
        host,
        ports,
    }
}

fn keeper(name: &'static str, deferred: Option<&'static str>, seen: &Arc<Seen>) -> Keeper {
    Keeper {
        name,
        deferred,
        seen: Arc::clone(seen),
    }
}

fn add_card(collection: &Collection) -> CardId {
    collection
        .run(OpSpec::named("add_notes", OpSource::UserUndoable), |ctx| {
            let nid = ctx.scope().add_note(&note(&["front", "back"]))?;
            Ok(ctx.scope().add_card(&Card {
                id: CardId(0),
                note_id: nid,
                deck_id: DeckId(1),
                ord: 0,
                mtime: EpochSecs(0),
                usn: Usn(0),
                card_type: CardType::New,
                queue: CardQueue::New,
                due: 1,
                interval: 0,
                ease_factor: 0,
                reps: 0,
                lapses: 0,
                remaining_steps: 0,
                original_due: 0,
                original_deck_id: DeckId(0),
                flags: 0,
                data: meguri_model::card_data::CardData::default(),
            })?)
        })
        .unwrap()
        .value
}

#[test]
fn d4_a_deferred_rebuild_waits_for_its_signal_and_holds_no_other_component() {
    let (_dir, path) = copy_fixture("schema-18.anki2");
    let (held, free) = (Arc::new(Seen::default()), Arc::new(Seen::default()));
    let opened = open(&path, |b| {
        b.component(keeper(KEPT, Some("homeIdle"), &held))
            .component(keeper("free", None, &free))
    });
    // The new file's rebuild verdict: the free component builds, the held
    // one waits, and the store counts as idle.
    opened.wait_ready("free");
    assert!(opened.host.wait_idle(WAIT));
    assert_eq!(opened.readiness(KEPT), Readiness::Stale);
    assert_eq!(held.builds.load(Ordering::SeqCst), 0);
    // A signal of another name changes nothing.
    opened.host.signal("somethingElse");
    assert!(opened.host.wait_idle(WAIT));
    assert_eq!(held.builds.load(Ordering::SeqCst), 0);
    opened.host.signal("homeIdle");
    opened.wait_ready(KEPT);
    assert_eq!(held.builds.load(Ordering::SeqCst), 1);
    // Released for the rest of the bind: an invalidation's rebuild starts
    // at once.
    opened.host.invalidate(meguri_engine::derived_host::Invalidation::CheckDatabase);
    assert!(opened.host.wait_idle(WAIT));
    opened.wait_ready(KEPT);
    assert_eq!(held.builds.load(Ordering::SeqCst), 2);
    // The next bind holds it again.
    opened.close();
    let _ = std::fs::remove_file(path.with_file_name("meguri-derived.db"));
    let again = Arc::new(Seen::default());
    let reopened = open(&path, |b| b.component(keeper(KEPT, Some("homeIdle"), &again)));
    assert!(reopened.host.wait_idle(WAIT));
    assert_eq!(reopened.readiness(KEPT), Readiness::Stale);
    // `request_refresh` releases it instead.
    reopened.host.request_refresh(KEPT);
    reopened.wait_ready(KEPT);
    assert_eq!(again.builds.load(Ordering::SeqCst), 1);
    reopened.close();
}

#[test]
fn d11_persist_runs_at_suspend_and_close_after_every_commit_sent() {
    let (_dir, path) = copy_fixture("schema-18.anki2");
    let seen = Arc::new(Seen::default());
    let opened = open(&path, |b| b.component(keeper(KEPT, None, &seen)));
    opened.wait_ready(KEPT);
    let before = opened.stored(KEPT);
    let first = add_card(&opened.collection);
    // Not persisted yet: in memory only.
    opened.host.flush().unwrap();
    assert_eq!(opened.stored(KEPT), before);
    let participant = opened.ports.suspend_participants[0].clone();
    participant.suspend().unwrap();
    let mut expected = before.clone();
    expected.push(first.0);
    expected.sort_unstable();
    assert_eq!(opened.stored(KEPT), expected);
    participant.resume().unwrap();
    let second = add_card(&opened.collection);
    expected.push(second.0);
    expected.sort_unstable();
    let persists = seen.persists.load(Ordering::SeqCst);
    opened.close();
    assert_eq!(seen.persists.load(Ordering::SeqCst), persists + 1);
    // The file holds the commit made just before the close, and is fresh.
    let again = Arc::new(Seen::default());
    let reopened = open(&path, |b| b.component(keeper(KEPT, None, &again)));
    assert!(reopened.host.wait_idle(WAIT));
    assert_eq!(reopened.readiness(KEPT), Readiness::Ready);
    assert_eq!(again.builds.load(Ordering::SeqCst), 0);
    assert_eq!(reopened.stored(KEPT), expected);
    reopened.close();
}

#[test]
fn d11_a_failing_persist_leaves_ready_0_and_the_next_bind_rebuilds() {
    let (_dir, path) = copy_fixture("schema-18.anki2");
    let seen = Arc::new(Seen::default());
    let opened = open(&path, |b| b.component(keeper(KEPT, None, &seen)));
    opened.wait_ready(KEPT);
    add_card(&opened.collection);
    opened.host.flush().unwrap();
    seen.fail_persist.store(true, Ordering::SeqCst);
    opened.close();
    let conn = rusqlite::Connection::open(path.with_file_name("meguri-derived.db")).unwrap();
    let (ready, clean): (i64, i64) = conn
        .query_row(
            "SELECT c.ready, s.clean FROM meguri_components AS c, meguri_stamp AS s WHERE c.name = ?1",
            [KEPT],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    drop(conn);
    assert_eq!((ready, clean), (0, 1));
    let again = Arc::new(Seen::default());
    let reopened = open(&path, |b| b.component(keeper(KEPT, None, &again)));
    reopened.wait_ready(KEPT);
    assert_eq!(again.builds.load(Ordering::SeqCst), 1);
    reopened.close();
}

#[test]
fn d4_release_persists_then_releases_and_a_failure_queues_a_rebuild() {
    let (_dir, path) = copy_fixture("schema-18.anki2");
    let (seen, other) = (Arc::new(Seen::default()), Arc::new(Seen::default()));
    let opened = open(&path, |b| {
        b.component(keeper(KEPT, None, &seen))
            .component(keeper("other", None, &other))
    });
    opened.wait_ready(KEPT);
    opened.wait_ready("other");
    let card = add_card(&opened.collection);
    opened.host.release(KEPT);
    assert_eq!(seen.releases.load(Ordering::SeqCst), 1);
    assert!(opened.stored(KEPT).contains(&card.0));
    assert!(seen.pending.lock().unwrap().is_empty());
    // The other component is unchanged.
    assert_eq!(other.persists.load(Ordering::SeqCst), 0);
    assert_eq!(other.releases.load(Ordering::SeqCst), 0);
    // A failed persist: `ready` 0 and a rebuild, and release still runs.
    let builds = seen.builds.load(Ordering::SeqCst);
    add_card(&opened.collection);
    seen.fail_persist.store(true, Ordering::SeqCst);
    opened.host.release(KEPT);
    assert_eq!(seen.releases.load(Ordering::SeqCst), 2);
    opened.wait_ready(KEPT);
    assert!(opened.host.wait_idle(WAIT));
    assert_eq!(seen.builds.load(Ordering::SeqCst), builds + 1);
    // A release of a name no component has changes nothing.
    opened.host.release("nothing");
    opened.close();
}
