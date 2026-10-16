//! The derived store as the engine runs it (`specs/storage/derived-store.md`
//! D5 to D12): binding at open, incremental updates from committed ops,
//! catch-up after a crash, rebuilds after a lost or damaged file and after a
//! collection swap, readiness on the change stream, and that none of it slows
//! the collection writer or touches the collection's schema.

mod common;

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use common::{NOW_MS, copy_fixture, note, open_config, revlog};
use meguri_engine::derived_host::{
    BuildSink, Capture, CatchUp, ComponentSchema, ComponentTables, DerivedComponent, DerivedHost, DerivedReadiness,
    IndexDef, Invalidation, Readiness, TableDef,
};
use meguri_engine::{
    Collection, Engine, EngineConfig, EngineError, Event, EventKind, Generation, LongOp, ObservedCommit, OpSpec, Ports,
    Qos, Snapshot, Subscription, monotonic,
};
use meguri_model::card::{Card, CardQueue, CardType};
use meguri_model::changes::OpSource;
use meguri_model::clock::{StepClock, UtcOffset};
use meguri_model::faultpoint::{self, Fault};
use meguri_model::ids::{CardId, DeckId, NoteId};
use meguri_model::time::{EpochMillis, EpochSecs};
use meguri_model::usn::Usn;
use meguri_storage::raw::SqlValue;
use meguri_storage::rusqlite;
use meguri_storage::{ReadRows, ReaderRole, Table};
use meguri_testkit::bench::fixtures;

const WAIT: Duration = Duration::from_secs(60);
const KILL_DIR: &str = "MEGURI_DERIVED_KILL_DIR";
const MIRROR: &str = "card_mirror";

/// One mirrored card: `(cid, nid, did, queue, due)`.
type Row = (i64, i64, i64, i64, i64);
/// The dues a reader of a generation saw.
type DuesSeen = (Generation, Vec<i64>);

fn row_of(card: &Card) -> Row {
    (
        card.id.0,
        card.note_id.0,
        card.deck_id.0,
        card.queue.stored(),
        i64::from(card.due),
    )
}

#[derive(Default)]
struct Counters {
    builds: AtomicUsize,
    catch_ups: AtomicUsize,
}

/// A test component: one row per card. It catches up from card
/// modification times (D4) and can be made to fail an apply or to build
/// slowly.
struct CardMirror {
    counters: Arc<Counters>,
    /// An apply of a row with this due fails.
    fail_on_due: Option<i64>,
    /// Cards per build batch, and a pause before each batch.
    batch: usize,
    pause: Duration,
}

impl CardMirror {
    fn new(counters: &Arc<Counters>) -> CardMirror {
        CardMirror {
            counters: Arc::clone(counters),
            fail_on_due: None,
            batch: 5_000,
            pause: Duration::ZERO,
        }
    }
}

enum Change {
    Put(Row),
    Remove(i64),
}

fn put_rows(tables: &ComponentTables<'_>, rows: &[Row]) -> Result<(), EngineError> {
    let table = tables.table("rows")?;
    let mut statement = tables
        .conn()
        .prepare_cached(&format!(
            "INSERT OR REPLACE INTO \"{table}\" (cid, nid, did, queue, due) VALUES (?1, ?2, ?3, ?4, ?5)"
        ))
        .map_err(sql)?;
    for row in rows {
        statement.execute(*row).map_err(sql)?;
    }
    Ok(())
}

fn sql(error: rusqlite::Error) -> EngineError {
    EngineError::Internal {
        message: error.to_string(),
    }
}

/// Every card of the snapshot, in id order, through `pages` of `batch`.
fn each_page(
    source: &Snapshot<'_>,
    batch: usize,
    mut page: impl FnMut(Vec<Card>) -> Result<(), EngineError>,
) -> Result<(), EngineError> {
    let mut after = None;
    loop {
        let cards = source.cards_after(after, batch)?;
        let Some(last) = cards.last() else { return Ok(()) };
        after = Some(last.id);
        page(cards)?;
    }
}

impl DerivedComponent for CardMirror {
    type Delta = Vec<Change>;

    fn schema(&self) -> ComponentSchema {
        ComponentSchema::new(MIRROR, 1)
            .table(TableDef::columns(
                "rows",
                "(cid INTEGER PRIMARY KEY, nid INTEGER NOT NULL, did INTEGER NOT NULL, \
                 queue INTEGER NOT NULL, due INTEGER NOT NULL)",
            ))
            .index(IndexDef::new("rows_nid", "rows", "(nid)"))
    }

    fn capture(&self, commit: &ObservedCommit<'_>) -> Capture<Vec<Change>> {
        if commit.cards.is_empty() {
            return Capture::Nothing;
        }
        Capture::Delta(
            commit
                .cards
                .iter()
                .map(|delta| match delta.after_card().unwrap() {
                    Some(card) => Change::Put(row_of(&card)),
                    None => Change::Remove(delta.id.0),
                })
                .collect(),
        )
    }

    fn apply(&self, tables: &ComponentTables<'_>, delta: Vec<Change>) -> Result<(), EngineError> {
        let table = tables.table("rows")?.to_owned();
        for change in delta {
            match change {
                Change::Put(row) => {
                    if Some(row.4) == self.fail_on_due {
                        return Err(EngineError::Internal {
                            message: "this due is refused".into(),
                        });
                    }
                    put_rows(tables, &[row])?;
                }
                Change::Remove(cid) => {
                    tables
                        .conn()
                        .execute(&format!("DELETE FROM \"{table}\" WHERE cid = ?1"), [cid])
                        .map_err(sql)?;
                }
            }
        }
        Ok(())
    }

    fn build(&self, source: &Snapshot<'_>, sink: &mut BuildSink) -> Result<(), EngineError> {
        self.counters.builds.fetch_add(1, Ordering::SeqCst);
        let total = match &source.execute_raw("SELECT count(*) FROM cards", &[])?.rows[0][0] {
            SqlValue::Integer(n) => *n as u64,
            other => panic!("{other:?}"),
        };
        let mut done = 0;
        each_page(source, self.batch, |cards| {
            thread::sleep(self.pause);
            let rows: Vec<Row> = cards.iter().map(row_of).collect();
            done += rows.len() as u64;
            sink.write(move |tables| put_rows(tables, &rows))?;
            sink.progress(done, Some(total));
            Ok(())
        })
    }

    fn catch_up(
        &self,
        source: &Snapshot<'_>,
        since: EpochMillis,
        sink: &mut BuildSink,
    ) -> Result<CatchUp, EngineError> {
        self.counters.catch_ups.fetch_add(1, Ordering::SeqCst);
        let mut changed = Vec::new();
        let mut present = HashSet::new();
        each_page(source, self.batch, |cards| {
            for card in cards {
                present.insert(card.id.0);
                if card.mtime >= since.whole_secs() {
                    changed.push(row_of(&card));
                }
            }
            Ok(())
        })?;
        sink.write(move |tables| {
            let table = tables.table("rows")?.to_owned();
            let stored: Vec<i64> = {
                let mut statement = tables
                    .conn()
                    .prepare(&format!("SELECT cid FROM \"{table}\""))
                    .map_err(sql)?;
                statement
                    .query_map([], |row| row.get(0))
                    .map_err(sql)?
                    .collect::<rusqlite::Result<_>>()
                    .map_err(sql)?
            };
            for cid in stored.into_iter().filter(|cid| !present.contains(cid)) {
                tables
                    .conn()
                    .execute(&format!("DELETE FROM \"{table}\" WHERE cid = ?1"), [cid])
                    .map_err(sql)?;
            }
            put_rows(tables, &changed)
        })?;
        Ok(CatchUp::Done)
    }
}

/// An engine with one collection open and the derived store wired as U38
/// wires it.
struct Opened {
    engine: Engine,
    collection: Collection,
    host: DerivedHost,
    events: Subscription,
    counters: Arc<Counters>,
    ports: Ports,
}

fn clock() -> Arc<StepClock> {
    Arc::new(StepClock::new(NOW_MS, UtcOffset::UTC, "UTC"))
}

fn open_with(path: &Path, mirror: impl FnOnce(&Arc<Counters>) -> CardMirror) -> Opened {
    let counters = Arc::new(Counters::default());
    let engine = Engine::new(EngineConfig::default()).unwrap();
    let events = engine.stream().subscribe();
    let host = DerivedHost::builder(&engine)
        .component(mirror(&counters))
        .start()
        .unwrap();
    let ports = host.ports(Ports::default());
    let collection = engine
        .open_raw(path, open_config(&clock()).ports(ports.clone()))
        .unwrap();
    Opened {
        engine,
        collection,
        host,
        events,
        counters,
        ports,
    }
}

fn open(path: &Path) -> Opened {
    open_with(path, CardMirror::new)
}

impl Opened {
    fn settle(&self) {
        assert!(self.host.wait_idle(WAIT), "the derived store did not settle");
    }

    fn close(self) {
        self.engine.close().unwrap();
        self.host.close().unwrap();
    }

    fn builds(&self) -> usize {
        self.counters.builds.load(Ordering::SeqCst)
    }

    fn catch_ups(&self) -> usize {
        self.counters.catch_ups.load(Ordering::SeqCst)
    }

    fn readiness(&self) -> Readiness {
        self.host.readiness(MIRROR).expect("registered").readiness
    }

    /// The readiness events received so far.
    fn readiness_events(&self) -> Vec<DerivedReadiness> {
        std::iter::from_fn(|| self.events.try_recv())
            .filter_map(|event| readiness_of(&event))
            .collect()
    }
}

fn readiness_of(event: &Event) -> Option<DerivedReadiness> {
    match &event.kind {
        EventKind::Extension(payload) => payload.as_any().downcast_ref::<DerivedReadiness>().cloned(),
        _ => None,
    }
}

/// The mirror's rows as a collection reader sees them through `derived`.
fn mirrored(collection: &Collection) -> Vec<Row> {
    let reader = collection.reader(ReaderRole::Query).unwrap();
    rows_of(
        &reader
            .execute_raw(
                "SELECT cid, nid, did, queue, due FROM derived.card_mirror__rows ORDER BY cid",
                &[],
            )
            .unwrap()
            .rows,
    )
}

fn rows_of(raw: &[Vec<SqlValue>]) -> Vec<Row> {
    let int = |v: &SqlValue| match v {
        SqlValue::Integer(n) => *n,
        other => panic!("{other:?}"),
    };
    raw.iter()
        .map(|r| (int(&r[0]), int(&r[1]), int(&r[2]), int(&r[3]), int(&r[4])))
        .collect()
}

/// What the mirror should hold: every card of the collection.
fn expected(collection: &Collection) -> Vec<Row> {
    let reader = collection.reader(ReaderRole::Query).unwrap();
    let mut rows = Vec::new();
    let mut after = None;
    loop {
        let cards = reader.cards_after(after, 5_000).unwrap();
        let Some(last) = cards.last() else { break };
        after = Some(last.id);
        rows.extend(cards.iter().map(row_of));
    }
    rows
}

fn card_with_due(note: NoteId, due: i64) -> Card {
    Card {
        id: CardId(0),
        note_id: note,
        deck_id: DeckId(1),
        ord: 0,
        mtime: EpochSecs(0),
        usn: Usn(0),
        card_type: CardType::New,
        queue: CardQueue::New,
        due: i32::try_from(due).unwrap(),
        interval: 0,
        ease_factor: 0,
        reps: 0,
        lapses: 0,
        remaining_steps: 0,
        original_due: 0,
        original_deck_id: DeckId(0),
        flags: 0,
        data: meguri_model::card_data::CardData::default(),
    }
}

/// Adds one note with one card per due, in one op.
fn add_cards(collection: &Collection, dues: &[i64]) -> Vec<CardId> {
    let dues = dues.to_vec();
    collection
        .run(OpSpec::named("add_notes", OpSource::UserUndoable), move |ctx| {
            dues.iter()
                .map(|due| {
                    let nid = ctx.scope().add_note(&note(&[&format!("n{due}"), "back"]))?;
                    Ok(ctx.scope().add_card(&card_with_due(nid, *due))?)
                })
                .collect()
        })
        .unwrap()
        .value
}

fn set_due(collection: &Collection, id: CardId, due: i64) {
    collection
        .run(OpSpec::named("set_due_date", OpSource::UserUndoable), move |ctx| {
            let mut card = ctx.scope().card(id)?.expect("the card exists");
            card.due = i32::try_from(due).unwrap();
            ctx.scope().put(&card)?;
            Ok(())
        })
        .unwrap();
}

fn remove_card(collection: &Collection, id: CardId) {
    collection
        .run(OpSpec::named("remove_cards", OpSource::UserUndoable), move |ctx| {
            ctx.scope().remove_card(id)?;
            Ok(())
        })
        .unwrap();
}

/// Runs `work` on the collection at `path` opened by an engine without the
/// derived store, then closes it.
fn without_store<T>(path: &Path, work: impl FnOnce(&Collection) -> T) -> T {
    without_store_at(path, NOW_MS, work)
}

/// [`without_store`] with the clock at `now_ms`.
fn without_store_at<T>(path: &Path, now_ms: i64, work: impl FnOnce(&Collection) -> T) -> T {
    let clock = clock();
    clock.set_now_ms(now_ms);
    let engine = Engine::new(EngineConfig::default()).unwrap();
    let collection = engine.open_raw(path, open_config(&clock)).unwrap();
    let out = work(&collection);
    drop(collection);
    engine.close().unwrap();
    out
}

fn derived_file(path: &Path) -> PathBuf {
    path.with_file_name("meguri-derived.db")
}

/// Every object of the collection's own schema.
fn collection_objects(collection: &Collection) -> Vec<Vec<SqlValue>> {
    let reader = collection.reader(ReaderRole::Query).unwrap();
    reader
        .execute_raw(
            "SELECT type, name, tbl_name, sql FROM main.sqlite_master ORDER BY type, name",
            &[],
        )
        .unwrap()
        .rows
}

#[test]
fn bind_builds_then_commits_flow_in_and_a_clean_reopen_is_fresh() {
    let (_dir, path) = copy_fixture("schema-18.anki2");
    let opened = open(&path);
    opened.settle();
    assert_eq!(opened.readiness(), Readiness::Ready);
    assert!(opened.host.is_ready(MIRROR, opened.collection.generation()));
    assert!(
        !opened
            .host
            .is_ready(MIRROR, Generation(opened.collection.generation().0 + 1))
    );
    assert_eq!(opened.builds(), 1, "a new file rebuilds");
    assert_eq!(mirrored(&opened.collection), expected(&opened.collection));

    let ids = add_cards(&opened.collection, &[11, 12, 13]);
    set_due(&opened.collection, ids[1], 99);
    remove_card(&opened.collection, ids[2]);
    opened.host.flush().unwrap();
    let rows = mirrored(&opened.collection);
    assert_eq!(rows, expected(&opened.collection));
    assert!(rows.iter().any(|r| r.0 == ids[1].0 && r.4 == 99));
    assert!(!rows.iter().any(|r| r.0 == ids[2].0));
    assert_eq!(opened.readiness(), Readiness::Ready, "no rebuild for normal commits");
    assert_eq!(opened.builds(), 1);
    let before = mirrored(&opened.collection);
    opened.close();

    let reopened = open(&path);
    reopened.settle();
    assert_eq!(reopened.readiness(), Readiness::Ready);
    assert_eq!((reopened.builds(), reopened.catch_ups()), (0, 0), "fresh");
    assert_eq!(mirrored(&reopened.collection), before);
    let events = reopened.readiness_events();
    assert!(
        events.iter().all(|e| e.readiness == Readiness::Ready),
        "a fresh store goes straight to Ready: {events:?}"
    );
    reopened.close();
}

#[test]
fn deleting_the_file_while_closed_rebuilds_the_same_content() {
    let (_dir, path) = copy_fixture("schema-18.anki2");
    let opened = open(&path);
    opened.settle();
    add_cards(&opened.collection, &[1, 2, 3, 4]);
    opened.host.flush().unwrap();
    let before = mirrored(&opened.collection);
    assert_eq!(before.len(), expected(&opened.collection).len());
    opened.close();

    let file = derived_file(&path);
    for suffix in ["", "-wal", "-shm"] {
        let mut name = file.as_os_str().to_owned();
        name.push(suffix);
        let _ = fs::remove_file(PathBuf::from(name));
    }
    let reopened = open(&path);
    reopened.settle();
    assert_eq!(reopened.builds(), 1, "rebuilt");
    assert_eq!(reopened.readiness(), Readiness::Ready);
    assert_eq!(mirrored(&reopened.collection), before);
    reopened.close();
}

#[test]
fn a_file_truncated_mid_page_is_recreated_at_open_and_rebuilt() {
    let (_dir, path) = copy_fixture("schema-18.anki2");
    let opened = open(&path);
    opened.settle();
    add_cards(&opened.collection, &(0..2_000).collect::<Vec<_>>());
    opened.host.flush().unwrap();
    let before = mirrored(&opened.collection);
    opened.close();

    let file = derived_file(&path);
    let size = fs::metadata(&file).unwrap().len();
    assert!(size >= 16 * 4096, "{size} bytes");
    let cut = (size / 4096 / 2) * 4096 + 1_000;
    fs::OpenOptions::new()
        .write(true)
        .open(&file)
        .unwrap()
        .set_len(cut)
        .unwrap();

    let reopened = open(&path);
    reopened.settle();
    assert_eq!(reopened.builds(), 1);
    assert_eq!(reopened.readiness(), Readiness::Ready);
    assert_eq!(mirrored(&reopened.collection), before);
    reopened.close();
}

/// Runs only as the child of `a_kill_between_commit_and_derived_update_...`:
/// commits one op the store applies, then dies between the next op's
/// collection commit and its derived update.
#[test]
fn child_killed_between_commit_and_derived_update() {
    let Some(dir) = std::env::var_os(KILL_DIR) else {
        return;
    };
    let path = Path::new(&dir).join("collection.anki2");
    let opened = open(&path);
    opened.settle();
    let ids = add_cards(&opened.collection, &[701, 702]);
    opened.host.flush().unwrap();
    if std::env::var_os("MEGURI_DERIVED_KILL_NOW").is_some() {
        // An unclean end with nothing left to apply.
        std::process::abort();
    }
    faultpoint::arm("engine.derived.before_apply", Fault::Abort);
    // One op adds, changes and removes cards; it commits, then the process
    // dies before the store applies it.
    let (changed, removed) = (ids[0], ids[1]);
    opened
        .collection
        .run(OpSpec::named("lost_update", OpSource::UserUndoable), move |ctx| {
            let nid = ctx.scope().add_note(&note(&["lost", "update"]))?;
            ctx.scope().add_card(&card_with_due(nid, 33))?;
            let mut card = ctx.scope().card(changed)?.expect("the card exists");
            card.due = 44;
            ctx.scope().put(&card)?;
            ctx.scope().remove_card(removed)?;
            Ok(())
        })
        .unwrap();
    thread::sleep(WAIT);
    unreachable!("the fault point did not end the process");
}

fn run_child(dir: &Path, now: bool) {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args([
            "--exact",
            "child_killed_between_commit_and_derived_update",
            "--nocapture",
        ])
        .env(KILL_DIR, dir);
    if now {
        command.env("MEGURI_DERIVED_KILL_NOW", "1");
    }
    let status = command.status().unwrap();
    assert!(!status.success(), "the child was not killed");
}

#[test]
fn a_kill_between_commit_and_derived_update_is_caught_up_to_what_a_rebuild_gives() {
    let (dir, path) = copy_fixture("schema-18.anki2");
    run_child(dir.path(), false);

    let reopened = open(&path);
    reopened.settle();
    assert_eq!(
        (reopened.catch_ups(), reopened.builds()),
        (1, 0),
        "lag found, caught up"
    );
    assert_eq!(reopened.readiness(), Readiness::Ready);
    let caught_up = mirrored(&reopened.collection);
    let dues: Vec<i64> = caught_up.iter().map(|r| r.4).collect();
    assert!(
        dues.contains(&33) && dues.contains(&44),
        "the lost update is there: {dues:?}"
    );
    assert!(
        !dues.contains(&702) && !dues.contains(&701),
        "the lost removal and change too: {dues:?}"
    );
    assert_eq!(caught_up, expected(&reopened.collection));

    reopened.host.invalidate(Invalidation::CheckDatabase);
    reopened.settle();
    assert_eq!(reopened.builds(), 1);
    assert_eq!(
        mirrored(&reopened.collection),
        caught_up,
        "a full rebuild gives the same"
    );
    reopened.close();
}

#[test]
fn an_unclean_end_with_no_lag_still_catches_up() {
    let (dir, path) = copy_fixture("schema-18.anki2");
    run_child(dir.path(), true);
    let reopened = open(&path);
    reopened.settle();
    assert_eq!((reopened.catch_ups(), reopened.builds()), (1, 0));
    assert_eq!(mirrored(&reopened.collection), expected(&reopened.collection));
    reopened.close();
}

#[test]
fn a_change_made_while_meguri_was_closed_rebuilds() {
    let (_dir, path) = copy_fixture("schema-18.anki2");
    let opened = open(&path);
    opened.settle();
    add_cards(&opened.collection, &[1]);
    opened.close();

    // Another engine without the store changes the collection, a minute
    // later.
    without_store_at(&path, NOW_MS + 60_000, |collection| add_cards(collection, &[2]));

    let reopened = open(&path);
    reopened.settle();
    assert_eq!((reopened.builds(), reopened.catch_ups()), (1, 0));
    assert_eq!(mirrored(&reopened.collection), expected(&reopened.collection));
    reopened.close();
}

#[test]
fn a_sync_incoming_commit_is_applied_incrementally() {
    let (_dir, path) = copy_fixture("schema-18.anki2");
    let opened = open(&path);
    opened.settle();
    let id = add_cards(&opened.collection, &[5])[0];
    opened.host.flush().unwrap();
    opened
        .collection
        .run(OpSpec::named("sync_incoming", OpSource::SyncIncoming), move |ctx| {
            let mut card = ctx.scope().card(id)?.unwrap();
            card.due = 500;
            card.usn = Usn(7);
            card.mtime = EpochSecs(NOW_MS / 1000 - 60);
            ctx.scope().put(&card)?;
            ctx.scope().set_col_mod(EpochMillis(NOW_MS + 5_000))?;
            Ok(())
        })
        .unwrap();
    opened.host.flush().unwrap();
    assert!(mirrored(&opened.collection).iter().any(|r| r.0 == id.0 && r.4 == 500));
    assert_eq!(opened.readiness(), Readiness::Ready);
    assert_eq!(opened.builds(), 1);
    opened.close();
}

#[test]
fn invalidation_rebuilds_through_staging_and_publishes_each_state() {
    let (_dir, path) = copy_fixture("schema-18.anki2");
    let opened = open(&path);
    opened.settle();
    add_cards(&opened.collection, &[1, 2]);
    opened.host.flush().unwrap();
    let before = mirrored(&opened.collection);
    opened.readiness_events();

    opened.host.invalidate(Invalidation::ExternalChange);
    opened.settle();
    assert_eq!(opened.builds(), 2);
    assert_eq!(mirrored(&opened.collection), before);
    let states: Vec<Readiness> = opened.readiness_events().into_iter().map(|e| e.readiness).collect();
    assert_eq!(states.first(), Some(&Readiness::Stale), "{states:?}");
    assert!(
        states.iter().any(|s| matches!(s, Readiness::Building { .. })),
        "{states:?}"
    );
    assert_eq!(states.last(), Some(&Readiness::Ready), "{states:?}");
    opened.close();
}

#[test]
fn a_failing_apply_makes_the_component_stale_and_rebuilds_it() {
    let (_dir, path) = copy_fixture("schema-18.anki2");
    let opened = open_with(&path, |counters| CardMirror {
        fail_on_due: Some(666),
        ..CardMirror::new(counters)
    });
    opened.settle();
    let id = add_cards(&opened.collection, &[1])[0];
    set_due(&opened.collection, id, 666);
    opened.settle();
    assert_eq!(opened.builds(), 2, "rebuilt after the failed apply");
    assert_eq!(opened.readiness(), Readiness::Ready);
    assert_eq!(mirrored(&opened.collection), expected(&opened.collection));
    let states: Vec<Readiness> = opened.readiness_events().into_iter().map(|e| e.readiness).collect();
    assert!(states.contains(&Readiness::Stale), "{states:?}");
    opened.close();
}

/// Makes the replacement file: the fixture with cards due 2000 and up, made
/// by an engine without the store.
fn replacement_with_cards(dir: &Path) -> PathBuf {
    let path = dir.join("replacement.anki2");
    fs::copy(common::fixture("schema-18.anki2"), &path).unwrap();
    without_store(&path, |collection| add_cards(collection, &[2_000, 2_001, 2_002]));
    path
}

#[test]
fn a_replaced_collection_is_rebuilt_and_no_reader_of_the_new_generation_sees_old_rows() {
    let (dir, path) = copy_fixture("schema-18.anki2");
    let replacement = replacement_with_cards(dir.path());
    let opened = open(&path);
    opened.settle();
    add_cards(&opened.collection, &[1_000, 1_001]);
    opened.host.flush().unwrap();
    assert!(mirrored(&opened.collection).iter().any(|r| r.4 == 1_000));
    let old = opened.collection.generation();

    // A reader that keeps reading through whatever handle is current.
    let stop = Arc::new(AtomicBool::new(false));
    let seen: Arc<Mutex<Vec<DuesSeen>>> = Arc::default();
    let poller = {
        let (engine, stop, seen) = (opened.engine.clone(), Arc::clone(&stop), Arc::clone(&seen));
        thread::spawn(move || {
            while !stop.load(Ordering::SeqCst) {
                let Ok(handle) = engine.collection() else { continue };
                let Ok(reader) = handle.reader(ReaderRole::Query) else {
                    continue;
                };
                if let Ok(raw) = reader.execute_raw("SELECT due FROM derived.card_mirror__rows", &[]) {
                    let dues = raw
                        .rows
                        .iter()
                        .map(|r| match r[0] {
                            SqlValue::Integer(due) => due,
                            ref other => panic!("{other:?}"),
                        })
                        .collect();
                    seen.lock().unwrap().push((handle.generation(), dues));
                }
            }
        })
    };

    let slot = opened.engine.acquire_slot(LongOp::Restore).unwrap();
    let mut exclusive = slot.enter_exclusive(false).unwrap();
    let source = replacement.clone();
    let new_handle = exclusive
        .replace(move |live| fs::rename(&source, live).map_err(|e| EngineError::Internal { message: e.to_string() }))
        .unwrap();
    let new = new_handle.generation();
    assert_eq!(new, Generation(old.0 + 1));
    assert!(
        !opened.host.is_ready(MIRROR, new),
        "not ready the moment the swap returns"
    );
    // The exclusive job goes on a while: the store cannot rebuild yet, and
    // the poller reads the new generation meanwhile.
    thread::sleep(Duration::from_millis(150));
    drop(exclusive);
    drop(slot);
    opened.settle();
    let current = opened.engine.collection().unwrap();
    assert!(opened.host.is_ready(MIRROR, new));
    let rows = mirrored(&current);
    assert_eq!(rows, expected(&current));
    let dues: Vec<i64> = rows.iter().map(|r| r.4).collect();
    assert!(
        [2_000, 2_001, 2_002].iter().all(|due| dues.contains(due))
            && !dues.iter().any(|due| (1_000..2_000).contains(due)),
        "{dues:?}"
    );
    // Let the poller read the new generation's rows a while.
    thread::sleep(Duration::from_millis(200));
    stop.store(true, Ordering::SeqCst);
    poller.join().unwrap();

    let seen = seen.lock().unwrap();
    assert!(
        seen.iter().any(|(g, _)| *g == new),
        "the poller read the new generation"
    );
    for (generation, dues) in seen.iter().filter(|(g, _)| *g == new) {
        assert!(
            dues.iter().all(|due| !(1_000..2_000).contains(due)),
            "generation {generation} saw an old row: {dues:?}"
        );
    }
    let events = opened.readiness_events();
    let after: Vec<&Readiness> = events
        .iter()
        .filter(|e| e.generation == new)
        .map(|e| &e.readiness)
        .collect();
    assert_eq!(after.first(), Some(&&Readiness::Stale), "{after:?}");
    assert_eq!(after.last(), Some(&&Readiness::Ready), "{after:?}");
    drop(current);
    opened.close();
}

#[test]
fn a_running_rebuild_yields_to_an_exclusive_phase_and_starts_again_after_it() {
    let (_dir, path) = copy_fixture("schema-18.anki2");
    // Some cards, so the slow build has batches to pause between.
    without_store(&path, |collection| add_cards(collection, &(0..60).collect::<Vec<_>>()));
    let opened = open_with(&path, |counters| CardMirror {
        batch: 2,
        pause: Duration::from_millis(30),
        ..CardMirror::new(counters)
    });
    let deadline = monotonic::now() + WAIT;
    while !matches!(opened.readiness(), Readiness::Building { .. }) {
        assert!(monotonic::now() < deadline, "the build never started");
        thread::sleep(Duration::from_millis(5));
    }
    // The exclusive job needs the maintenance reader (its backup): the build
    // lets go of it at its next batch.
    let slot = opened.engine.acquire_slot(LongOp::Restore).unwrap();
    let exclusive = slot.enter_exclusive(true).unwrap();
    let started = monotonic::now();
    let lease = exclusive.collection().reader(ReaderRole::Maintenance).unwrap();
    assert!(
        monotonic::now() - started < Duration::from_secs(1),
        "the build held the maintenance reader for {:?}",
        monotonic::now() - started
    );
    drop(lease);
    // Cancelled without a replacement: the build starts again.
    drop(exclusive);
    drop(slot);
    opened.settle();
    assert!(opened.builds() >= 2, "{} builds", opened.builds());
    assert_eq!(opened.readiness(), Readiness::Ready);
    assert_eq!(mirrored(&opened.collection), expected(&opened.collection));
    opened.close();
}

#[test]
fn suspend_marks_the_store_clean_and_resume_marks_it_in_use() {
    let (_dir, path) = copy_fixture("schema-18.anki2");
    let opened = open(&path);
    opened.settle();
    let clean = |collection: &Collection| {
        let reader = collection.reader(ReaderRole::Query).unwrap();
        match &reader
            .execute_raw("SELECT clean FROM derived.meguri_stamp", &[])
            .unwrap()
            .rows[0][0]
        {
            SqlValue::Integer(v) => *v,
            other => panic!("{other:?}"),
        }
    };
    assert_eq!(clean(&opened.collection), 0, "in use");
    let participant = opened.ports.suspend_participants[0].clone();
    participant.suspend().unwrap();
    assert_eq!(clean(&opened.collection), 1);
    participant.resume().unwrap();
    assert_eq!(clean(&opened.collection), 0);
    add_cards(&opened.collection, &[1]);
    participant.flush().unwrap();
    assert_eq!(mirrored(&opened.collection), expected(&opened.collection));
    opened.close();
}

#[test]
fn the_derived_writer_runs_at_utility_and_never_touches_the_collection_schema() {
    let (_dir, path) = copy_fixture("schema-18.anki2");
    let before = without_store(&path, collection_objects);
    let opened = open(&path);
    opened.settle();
    let writer = opened
        .engine
        .threads()
        .threads()
        .into_iter()
        .find(|t| t.name == "meguri-derived-writer")
        .expect("the derived writer is registered");
    assert_eq!(writer.qos, Qos::Utility);
    if cfg!(target_vendor = "apple") {
        assert_eq!(writer.reported_qos, Some(Qos::Utility));
    }
    let ids = add_cards(&opened.collection, &[1, 2, 3]);
    set_due(&opened.collection, ids[0], 9);
    opened.host.invalidate(Invalidation::CheckDatabase);
    opened.settle();
    assert_eq!(collection_objects(&opened.collection), before);
    opened.close();
    assert_eq!(without_store(&path, collection_objects), before);
}

/// The p-th percentile of `samples`.
fn percentile(mut samples: Vec<Duration>, p: usize) -> Duration {
    samples.sort();
    let rank = (samples.len() * p / 100 + 1).min(samples.len());
    samples[rank - 1]
}

#[test]
fn a_rebuild_over_150k_cards_leaves_answers_fast_and_reports_its_progress() {
    let generated = fixtures::profile_collection("150k").expect("the 150k collection");
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("collection.anki2");
    fs::copy(generated.dir.join("collection.anki2"), &path).unwrap();
    let opened = open(&path);
    let collection = &opened.collection;
    let card_ids: Vec<i64> = collection
        .reader(ReaderRole::Query)
        .unwrap()
        .ids_after(Table::Cards, None, usize::MAX)
        .unwrap();
    assert!(card_ids.len() >= 140_000, "{} cards", card_ids.len());

    let answer = |i: usize| {
        let id = CardId(card_ids[(i * 7_919) % card_ids.len()]);
        collection
            .run(OpSpec::named("answer_card", OpSource::UserUndoable), move |ctx| {
                let mut card = ctx.scope().card(id)?.expect("the card exists");
                card.due += 1;
                card.reps += 1;
                ctx.scope().put(&card)?;
                ctx.scope().add_revlog(&revlog(id))?;
                Ok(())
            })
            .unwrap()
            .timing
            .write
    };
    let mut during = Vec::new();
    let mut i = 0;
    let deadline = monotonic::now() + Duration::from_secs(600);
    while opened.readiness() != Readiness::Ready {
        assert!(monotonic::now() < deadline, "the rebuild did not end");
        let building = matches!(opened.readiness(), Readiness::Building { .. });
        let write = answer(i);
        i += 1;
        if building && matches!(opened.readiness(), Readiness::Building { .. }) {
            during.push(write);
        }
    }
    let mut after = Vec::new();
    for _ in 0..during.len().clamp(200, 2_000) {
        after.push(answer(i));
        i += 1;
    }
    assert!(
        during.len() >= 50,
        "only {} answers ran during the rebuild",
        during.len()
    );
    let (p95_during, p95_after) = (percentile(during.clone(), 95), percentile(after, 95));
    eprintln!(
        "answer write phase p95: {p95_during:?} during the rebuild ({} answers), {p95_after:?} after",
        during.len()
    );
    assert!(
        p95_during < Duration::from_millis(5),
        "answer write phase p95 {p95_during:?} during the rebuild ({} answers), {p95_after:?} after",
        during.len()
    );

    opened.settle();
    let events = opened.readiness_events();
    let progress: Vec<(u64, Option<u64>)> = events
        .iter()
        .filter_map(|e| match e.readiness {
            Readiness::Building { completed, total } => Some((completed, total)),
            _ => None,
        })
        .collect();
    assert!(progress.len() >= 2, "{progress:?}");
    assert!(progress.windows(2).all(|w| w[0].0 <= w[1].0), "{progress:?}");
    assert!(
        progress.iter().any(|(_, total)| *total == Some(card_ids.len() as u64)),
        "{progress:?}"
    );
    assert_eq!(events.last().map(|e| &e.readiness), Some(&Readiness::Ready));
    let rows = mirrored(collection);
    assert_eq!(rows.len(), card_ids.len());
    assert_eq!(rows, expected(collection));
    opened.close();
}
