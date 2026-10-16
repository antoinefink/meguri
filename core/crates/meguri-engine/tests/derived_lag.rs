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
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::Duration;

use common::{NOW_MS, copy_fixture, note, open_config, revlog};
use meguri_engine::derived_host::{
    BuildSink, Capture, CatchUp, ComponentSchema, ComponentTables, DerivedApplied, DerivedComponent, DerivedHost,
    DerivedHostBuilder, DerivedReadiness, IndexDef, Invalidation, Readiness, TableDef,
};
use meguri_engine::{
    Collection, Engine, EngineConfig, EngineError, Event, EventKind, Generation, LongOp, ObservedCommit, OpSpec, Ports,
    Qos, Seq, Snapshot, Subscription, monotonic,
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

/// Holds a build after its first batch until a test opens it.
#[derive(Default)]
struct Gate {
    open: Mutex<bool>,
    changed: Condvar,
}

impl Gate {
    fn open(&self) {
        *self.open.lock().unwrap() = true;
        self.changed.notify_all();
    }

    fn is_open(&self) -> bool {
        *self.open.lock().unwrap()
    }

    /// Waits until the gate opens, at most [`WAIT`].
    fn pass(&self) {
        let open = self.open.lock().unwrap();
        let _open = self.changed.wait_timeout_while(open, WAIT, |open| !*open).unwrap();
    }
}

/// A test component: one row per card. It catches up from card
/// modification times (D4) and can be made to fail or panic in an apply, to
/// hold an apply, to ask for a rebuild, to panic in its first build's first
/// batch, to build slowly, or to stop its builds or catch-ups until a gate
/// opens.
struct CardMirror {
    counters: Arc<Counters>,
    /// An apply of a row with this due fails.
    fail_on_due: Option<i64>,
    /// An apply of a row with this due panics.
    panic_on_due: Option<i64>,
    /// An apply of a row with this due waits for the gate first, holding
    /// the derived writer.
    held_apply: Option<(i64, Arc<Gate>)>,
    /// A commit that gives a card this due is too much to apply: its
    /// capture asks for a rebuild (D4).
    rebuild_on_due: Option<i64>,
    /// The first build's first batch panics on the derived writer.
    panic_in_first_batch: bool,
    /// The build goes on after a batch failed (against D4's contract).
    ignore_batch_errors: bool,
    /// Every build waits here after its first batch.
    gate: Option<Arc<Gate>>,
    /// Every catch-up waits here before its write, its snapshot taken.
    catch_up_gate: Option<Arc<Gate>>,
    /// Cards per build batch, and a pause before each batch.
    batch: usize,
    pause: Duration,
}

impl CardMirror {
    fn new(counters: &Arc<Counters>) -> CardMirror {
        CardMirror {
            counters: Arc::clone(counters),
            fail_on_due: None,
            panic_on_due: None,
            held_apply: None,
            rebuild_on_due: None,
            panic_in_first_batch: false,
            ignore_batch_errors: false,
            gate: None,
            catch_up_gate: None,
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
        let changes: Vec<Change> = commit
            .cards
            .iter()
            .map(|delta| match delta.after_card().unwrap() {
                Some(card) => Change::Put(row_of(&card)),
                None => Change::Remove(delta.id.0),
            })
            .collect();
        let too_much = changes
            .iter()
            .any(|change| matches!(change, Change::Put(row) if Some(row.4) == self.rebuild_on_due));
        if too_much {
            return Capture::Rebuild;
        }
        Capture::Delta(changes)
    }

    fn apply(&self, tables: &ComponentTables<'_>, delta: Vec<Change>) -> Result<(), EngineError> {
        let table = tables.table("rows")?.to_owned();
        for change in delta {
            match change {
                Change::Put(row) => {
                    if let Some((due, gate)) = &self.held_apply
                        && row.4 == *due
                    {
                        gate.pass();
                    }
                    if Some(row.4) == self.fail_on_due {
                        return Err(EngineError::Internal {
                            message: "this due is refused".into(),
                        });
                    }
                    // Half written when it panics.
                    put_rows(tables, &[row])?;
                    if Some(row.4) == self.panic_on_due {
                        panic!("an apply panics on due {}", row.4);
                    }
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
        let first_build = self.counters.builds.fetch_add(1, Ordering::SeqCst) == 0;
        let total = match &source.execute_raw("SELECT count(*) FROM cards", &[])?.rows[0][0] {
            SqlValue::Integer(n) => *n as u64,
            other => panic!("{other:?}"),
        };
        let mut done = 0;
        let mut first_batch = true;
        each_page(source, self.batch, |cards| {
            thread::sleep(self.pause);
            let rows: Vec<Row> = cards.iter().map(row_of).collect();
            done += rows.len() as u64;
            let panics = self.panic_in_first_batch && first_build && first_batch;
            let written = sink.write(move |tables| {
                // Half written when it panics.
                put_rows(tables, &rows[..rows.len() / 2])?;
                if panics {
                    panic!("a build batch panics");
                }
                put_rows(tables, &rows[rows.len() / 2..])
            });
            if !self.ignore_batch_errors {
                written?;
            }
            sink.progress(done, Some(total));
            if first_batch && let Some(gate) = &self.gate {
                gate.pass();
            }
            first_batch = false;
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
        if let Some(gate) = &self.catch_up_gate {
            gate.pass();
        }
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

/// A component whose build holds the one refresh the store runs at a time
/// until a gate opens, so the refreshes queued after it wait.
struct Blocker {
    gate: Arc<Gate>,
    started: Arc<AtomicBool>,
}

impl DerivedComponent for Blocker {
    type Delta = ();

    fn schema(&self) -> ComponentSchema {
        ComponentSchema::new("blocker", 1).table(TableDef::columns("rows", "(id INTEGER PRIMARY KEY)"))
    }

    fn capture(&self, _commit: &ObservedCommit<'_>) -> Capture<()> {
        Capture::Nothing
    }

    fn apply(&self, _tables: &ComponentTables<'_>, _delta: ()) -> Result<(), EngineError> {
        Ok(())
    }

    fn build(&self, _source: &Snapshot<'_>, _sink: &mut BuildSink) -> Result<(), EngineError> {
        self.started.store(true, Ordering::SeqCst);
        self.gate.pass();
        Ok(())
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
    open_components(path, |builder, counters| builder.component(mirror(counters)))
}

/// Opens with the components `register` adds, in its order.
fn open_components(
    path: &Path,
    register: impl FnOnce(DerivedHostBuilder, &Arc<Counters>) -> DerivedHostBuilder,
) -> Opened {
    let counters = Arc::new(Counters::default());
    let engine = Engine::new(EngineConfig::default()).unwrap();
    let events = engine.stream().subscribe();
    let host = register(DerivedHost::builder(&engine), &counters).start().unwrap();
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
        self.readiness_envelopes().into_iter().map(|(_, event)| event).collect()
    }

    /// The readiness events received so far, with their envelopes'
    /// `cause_seq`.
    fn readiness_envelopes(&self) -> Vec<(Option<Seq>, DerivedReadiness)> {
        std::iter::from_fn(|| self.events.try_recv())
            .filter_map(|event| readiness_of(&event).map(|readiness| (event.cause_seq, readiness)))
            .collect()
    }

    /// Waits until a build has written its first batch: its snapshot is
    /// taken.
    fn wait_for_first_batch(&self) {
        let deadline = monotonic::now() + WAIT;
        while !matches!(self.readiness(), Readiness::Building { completed, .. } if completed > 0) {
            assert!(monotonic::now() < deadline, "the build never wrote a batch");
            thread::sleep(Duration::from_millis(2));
        }
    }
}

fn readiness_of(event: &Event) -> Option<DerivedReadiness> {
    match &event.kind {
        EventKind::Extension(payload) => payload.as_any().downcast_ref::<DerivedReadiness>().cloned(),
        _ => None,
    }
}

fn applied_of(event: &Event) -> Option<DerivedApplied> {
    match &event.kind {
        EventKind::Extension(payload) => payload.as_any().downcast_ref::<DerivedApplied>().cloned(),
        _ => None,
    }
}

/// Takes events until one matches, waiting at most [`WAIT`] in all.
fn wait_for_event(events: &Subscription, mut wanted: impl FnMut(&Event) -> bool) -> Arc<Event> {
    let deadline = monotonic::now() + WAIT;
    loop {
        let left = deadline.saturating_sub(monotonic::now());
        let event = events.recv_timeout(left).expect("the event never came");
        if wanted(&event) {
            return event;
        }
    }
}

/// Polls until `done`, at most [`WAIT`].
fn wait_until(what: &str, done: impl Fn() -> bool) {
    let deadline = monotonic::now() + WAIT;
    while !done() {
        assert!(monotonic::now() < deadline, "{what} never happened");
        thread::sleep(Duration::from_millis(2));
    }
}

/// The mirror's rows as `snapshot` reads them through `derived`.
fn mirrored_in(snapshot: &Snapshot<'_>) -> Vec<Row> {
    rows_of(
        &snapshot
            .execute_raw(
                "SELECT cid, nid, did, queue, due FROM derived.card_mirror__rows ORDER BY cid",
                &[],
            )
            .unwrap()
            .rows,
    )
}

/// Every card as `snapshot` reads the collection, as mirror rows.
fn cards_in(snapshot: &Snapshot<'_>) -> Vec<Row> {
    let mut rows = Vec::new();
    each_page(snapshot, 5_000, |cards| {
        rows.extend(cards.iter().map(row_of));
        Ok(())
    })
    .unwrap();
    rows
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

/// The stamp's `clean` as a collection reader sees it.
fn stamp_clean(collection: &Collection) -> i64 {
    let reader = collection.reader(ReaderRole::Query).unwrap();
    match &reader
        .execute_raw("SELECT clean FROM derived.meguri_stamp", &[])
        .unwrap()
        .rows[0][0]
    {
        SqlValue::Integer(v) => *v,
        other => panic!("{other:?}"),
    }
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

/// Sets a card's due in one op; returns the op's seq.
fn set_due(collection: &Collection, id: CardId, due: i64) -> Seq {
    collection
        .run(OpSpec::named("set_due_date", OpSource::UserUndoable), move |ctx| {
            let mut card = ctx.scope().card(id)?.expect("the card exists");
            card.due = i32::try_from(due).unwrap();
            ctx.scope().put(&card)?;
            Ok(())
        })
        .unwrap()
        .seq
}

/// A sync brings a change to a card made a day ago: its modification time
/// is before the `since` of any catch-up. Returns the op's seq.
fn sync_change(collection: &Collection, id: CardId, due: i64) -> Seq {
    collection
        .run(OpSpec::named("sync_incoming", OpSource::SyncIncoming), move |ctx| {
            let mut card = ctx.scope().card(id)?.expect("the card exists");
            card.due = i32::try_from(due).unwrap();
            card.usn = Usn(7);
            card.mtime = EpochSecs(NOW_MS / 1000 - 86_400);
            ctx.scope().put(&card)?;
            Ok(())
        })
        .unwrap()
        .seq
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

/// Leaves the closed store as a crash with nothing left to apply leaves it:
/// `clean` 0, so the next bind catches up (D5.2).
fn mark_unclean(path: &Path) {
    let conn = rusqlite::Connection::open(derived_file(path)).unwrap();
    assert_eq!(conn.execute("UPDATE meguri_stamp SET clean = 0", []).unwrap(), 1);
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

/// A store left unclean over a collection with two cards: the next bind
/// catches the mirror up (D5.2).
fn unclean_store_with_two_cards(path: &Path) -> Vec<CardId> {
    let opened = open(path);
    opened.settle();
    let ids = add_cards(&opened.collection, &[1, 2]);
    opened.close();
    mark_unclean(path);
    ids
}

#[test]
fn a_rebuild_capture_during_a_running_catch_up_starts_it_again_as_a_rebuild() {
    let (_dir, path) = copy_fixture("schema-18.anki2");
    let ids = unclean_store_with_two_cards(&path);
    let gate = Arc::new(Gate::default());
    let reopened = open_with(&path, |counters| CardMirror {
        rebuild_on_due: Some(9_000),
        catch_up_gate: Some(Arc::clone(&gate)),
        ..CardMirror::new(counters)
    });
    // The catch-up has its snapshot and waits before its write.
    wait_until("the catch-up", || reopened.catch_ups() == 1);
    // A commit after that snapshot asks for a rebuild; a catch-up from
    // modification times would not see it.
    sync_change(&reopened.collection, ids[0], 9_000);
    reopened.host.flush().unwrap();
    gate.open();
    reopened.settle();
    assert_eq!(reopened.readiness(), Readiness::Ready);
    let rows = mirrored(&reopened.collection);
    assert!(
        rows.iter().any(|r| r.0 == ids[0].0 && r.4 == 9_000),
        "the synced change is there: {rows:?}"
    );
    assert_eq!(rows, expected(&reopened.collection));
    assert_eq!(
        (reopened.catch_ups(), reopened.builds()),
        (1, 1),
        "the catch-up started again as a rebuild"
    );
    reopened.close();
}

#[test]
fn a_rebuild_capture_while_a_catch_up_waits_its_turn_makes_it_a_rebuild() {
    let (_dir, path) = copy_fixture("schema-18.anki2");
    let ids = unclean_store_with_two_cards(&path);
    let gate = Arc::new(Gate::default());
    let started = Arc::new(AtomicBool::new(false));
    // The new blocker's rebuild runs first and holds the store's one
    // refresh; the mirror's catch-up waits behind it.
    let reopened = open_components(&path, |builder, counters| {
        builder
            .component(Blocker {
                gate: Arc::clone(&gate),
                started: Arc::clone(&started),
            })
            .component(CardMirror {
                rebuild_on_due: Some(9_000),
                ..CardMirror::new(counters)
            })
    });
    wait_until("the blocker's rebuild", || started.load(Ordering::SeqCst));
    assert_eq!(reopened.readiness(), Readiness::Stale, "the catch-up waits");
    sync_change(&reopened.collection, ids[0], 9_000);
    reopened.host.flush().unwrap();
    gate.open();
    reopened.settle();
    assert_eq!(reopened.readiness(), Readiness::Ready);
    let rows = mirrored(&reopened.collection);
    assert!(
        rows.iter().any(|r| r.0 == ids[0].0 && r.4 == 9_000),
        "the synced change is there: {rows:?}"
    );
    assert_eq!(rows, expected(&reopened.collection));
    assert_eq!(
        (reopened.catch_ups(), reopened.builds()),
        (0, 1),
        "the waiting catch-up became a rebuild"
    );
    reopened.close();
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

#[test]
fn readiness_changed_by_a_commit_carries_that_commits_seq() {
    let (_dir, path) = copy_fixture("schema-18.anki2");
    let opened = open_with(&path, |counters| CardMirror {
        fail_on_due: Some(666),
        ..CardMirror::new(counters)
    });
    opened.settle();
    let at_open = opened.readiness_envelopes();
    assert!(
        at_open.last().is_some_and(|(_, e)| e.readiness == Readiness::Ready),
        "{at_open:?}"
    );
    assert!(
        at_open.iter().all(|(cause, _)| cause.is_none()),
        "binding at open is no commit's doing: {at_open:?}"
    );
    let id = add_cards(&opened.collection, &[1])[0];
    let failed = set_due(&opened.collection, id, 666);
    opened.settle();
    let events = opened.readiness_envelopes();
    let states: Vec<&Readiness> = events.iter().map(|(_, e)| &e.readiness).collect();
    assert_eq!(states.first(), Some(&&Readiness::Stale), "{events:?}");
    assert!(
        states.iter().any(|s| matches!(s, Readiness::Building { .. })),
        "{events:?}"
    );
    assert_eq!(states.last(), Some(&&Readiness::Ready), "{events:?}");
    assert!(
        events.iter().all(|(cause, _)| *cause == Some(failed)),
        "every state of the rebuild names the commit whose apply failed ({failed:?}): {events:?}"
    );
    opened.close();
}

#[test]
fn a_commit_the_store_has_not_applied_is_not_current_and_its_application_is_published() {
    let (_dir, path) = copy_fixture("schema-18.anki2");
    let gate = Arc::new(Gate::default());
    let opened = open_with(&path, |counters| CardMirror {
        held_apply: Some((888, Arc::clone(&gate))),
        ..CardMirror::new(counters)
    });
    opened.settle();
    let generation = opened.collection.generation();
    let id = add_cards(&opened.collection, &[1])[0];
    opened.host.flush().unwrap();
    let lease = opened.collection.reader(ReaderRole::Query).unwrap();
    {
        let snapshot = lease.snapshot().unwrap();
        assert!(opened.host.is_current(MIRROR, &snapshot), "every commit is applied");
    }
    // An op with an empty change set is never sent to the store: the store
    // stays current for a snapshot that ends with it.
    let empty = opened
        .collection
        .run(OpSpec::named("nothing", OpSource::UserUndoable), |_| Ok(()))
        .unwrap();
    assert!(empty.changes.is_empty());
    {
        let snapshot = lease.snapshot().unwrap();
        assert_eq!(snapshot.seq(), empty.seq);
        assert!(opened.host.is_current(MIRROR, &snapshot), "nothing to apply");
    }

    // The derived writer is held in this commit's apply.
    let seq = set_due(&opened.collection, id, 888);
    // A consumer reacts to the commit's change set.
    wait_for_event(&opened.events, |e| {
        e.seq == seq && matches!(e.kind, EventKind::Changes(_))
    });
    let early = lease.snapshot().unwrap();
    assert_eq!(early.seq(), seq);
    assert!(
        opened.host.is_ready(MIRROR, generation),
        "Ready alone says nothing of the lag"
    );
    assert!(
        !opened.host.is_current(MIRROR, &early),
        "the commit is not in the store yet"
    );
    assert!(
        mirrored_in(&early).iter().any(|r| r.0 == id.0 && r.4 == 1),
        "the snapshot's derived rows are behind"
    );

    gate.open();
    let event = wait_for_event(&opened.events, |e| applied_of(e).is_some_and(|a| a.through >= seq));
    assert_eq!(
        applied_of(&event),
        Some(DerivedApplied {
            generation,
            through: seq
        })
    );
    assert_eq!(event.cause_seq, Some(seq));
    assert!(
        !opened.host.is_current(MIRROR, &early),
        "a snapshot that read the store before the apply still reads it from before"
    );
    drop(early);
    let late = lease.snapshot().unwrap();
    assert!(opened.host.is_current(MIRROR, &late));
    assert!(mirrored_in(&late).iter().any(|r| r.0 == id.0 && r.4 == 888));
    drop(late);
    drop(lease);
    opened.close();
}

#[test]
fn a_store_that_applied_a_commit_after_the_snapshot_before_its_first_derived_read_is_not_current() {
    let (_dir, path) = copy_fixture("schema-18.anki2");
    let opened = open(&path);
    opened.settle();
    let generation = opened.collection.generation();
    let id = add_cards(&opened.collection, &[1])[0];
    opened.host.flush().unwrap();
    // Two consumers' snapshots of the same commit: one asks at once, the
    // other has not read `derived` yet.
    let asked_lease = opened.collection.reader(ReaderRole::Interactive).unwrap();
    let asked = asked_lease.snapshot().unwrap();
    assert!(opened.host.is_current(MIRROR, &asked));
    let unread_lease = opened.collection.reader(ReaderRole::Query).unwrap();
    let unread = unread_lease.snapshot().unwrap();
    assert_eq!(unread.seq(), asked.seq());

    // A later commit is applied to the store.
    let later = set_due(&opened.collection, id, 2);
    assert!(later > unread.seq());
    wait_for_event(&opened.events, |e| applied_of(e).is_some_and(|a| a.through >= later));

    for snapshot in [&asked, &unread] {
        assert!(
            cards_in(snapshot).iter().any(|r| r.0 == id.0 && r.4 == 1),
            "each snapshot sees the collection before the later commit"
        );
    }
    assert!(
        opened.host.is_current(MIRROR, &asked),
        "the store as the snapshot first read it"
    );
    assert_eq!(mirrored_in(&asked), cards_in(&asked));

    assert!(opened.host.is_ready(MIRROR, generation));
    assert!(
        !opened.host.is_current(MIRROR, &unread),
        "the snapshot's first read of `derived` sees a commit its collection does not"
    );
    assert_ne!(mirrored_in(&unread), cards_in(&unread), "its derived rows are ahead");
    drop(unread);
    drop(asked);

    let late = unread_lease.snapshot().unwrap();
    assert!(opened.host.is_current(MIRROR, &late));
    assert!(mirrored_in(&late).iter().any(|r| r.0 == id.0 && r.4 == 2));
    assert_eq!(mirrored_in(&late), cards_in(&late));
    drop(late);
    drop(unread_lease);
    drop(asked_lease);
    opened.close();
}

#[test]
fn a_commit_batch_that_fails_while_a_rebuild_runs_still_reaches_the_rebuilt_tables() {
    let (_dir, path) = copy_fixture("schema-18.anki2");
    let ids = without_store(&path, |collection| add_cards(collection, &(0..10).collect::<Vec<_>>()));
    let gate = Arc::new(Gate::default());
    let opened = open_with(&path, |counters| CardMirror {
        batch: 2,
        gate: Some(Arc::clone(&gate)),
        ..CardMirror::new(counters)
    });
    opened.wait_for_first_batch();
    // Commits after the build's snapshot, whose derived transactions fail.
    faultpoint::arm("engine.derived.before_apply", Fault::IoError(std::io::ErrorKind::Other));
    set_due(&opened.collection, ids[0], 4_242);
    let added = add_cards(&opened.collection, &[4_343])[0];
    remove_card(&opened.collection, ids[1]);
    opened.host.flush().unwrap();
    faultpoint::disarm("engine.derived.before_apply");
    assert!(
        matches!(opened.readiness(), Readiness::Building { .. }),
        "the build still runs: {:?}",
        opened.readiness()
    );
    gate.open();
    opened.settle();
    assert_eq!(opened.readiness(), Readiness::Ready);
    let rows = mirrored(&opened.collection);
    assert!(
        rows.iter().any(|r| r.0 == ids[0].0 && r.4 == 4_242),
        "the change is there"
    );
    assert!(rows.iter().any(|r| r.0 == added.0 && r.4 == 4_343), "the addition too");
    assert!(!rows.iter().any(|r| r.0 == ids[1].0), "and the removal");
    assert_eq!(rows, expected(&opened.collection));
    opened.close();
}

#[test]
fn suspend_returns_only_once_a_running_build_let_go_of_its_snapshot() {
    let (_dir, path) = copy_fixture("schema-18.anki2");
    without_store(&path, |collection| add_cards(collection, &(0..10).collect::<Vec<_>>()));
    let gate = Arc::new(Gate::default());
    let opened = open_with(&path, |counters| CardMirror {
        batch: 2,
        gate: Some(Arc::clone(&gate)),
        ..CardMirror::new(counters)
    });
    // The builder holds its snapshot, stopped after its first batch.
    opened.wait_for_first_batch();
    let participant = opened.ports.suspend_participants[0].clone();
    let suspended = {
        let (engine, gate, participant) = (opened.engine.clone(), Arc::clone(&gate), participant.clone());
        thread::spawn(move || {
            participant.suspend().unwrap();
            // What still runs the moment suspend returns.
            let builders = engine
                .threads()
                .busy_threads()
                .into_iter()
                .filter(|t| t.name == "meguri-derived-builder")
                .count();
            (gate.is_open(), builders)
        })
    };
    thread::sleep(Duration::from_millis(200));
    // The builder reaches its next batch, where it stops.
    gate.open();
    let (gate_was_open, builders) = suspended.join().unwrap();
    assert!(gate_was_open, "suspend returned while the builder held its snapshot");
    assert_eq!(builders, 0, "a builder was still busy after suspend");
    let started = monotonic::now();
    drop(opened.collection.reader(ReaderRole::Maintenance).unwrap());
    assert!(
        monotonic::now() - started < Duration::from_millis(500),
        "the maintenance reader was held for {:?} after suspend",
        monotonic::now() - started
    );
    assert_eq!(stamp_clean(&opened.collection), 1, "suspended clean");

    // The build starts again on resume.
    participant.resume().unwrap();
    opened.settle();
    assert_eq!(opened.readiness(), Readiness::Ready);
    assert!(opened.builds() >= 2, "{} builds", opened.builds());
    assert_eq!(mirrored(&opened.collection), expected(&opened.collection));
    opened.close();
}

#[test]
fn a_panicking_apply_is_rolled_back_and_rebuilt_and_the_writer_goes_on() {
    let (_dir, path) = copy_fixture("schema-18.anki2");
    let opened = open_with(&path, |counters| CardMirror {
        panic_on_due: Some(667),
        ..CardMirror::new(counters)
    });
    opened.settle();
    let id = add_cards(&opened.collection, &[1])[0];
    set_due(&opened.collection, id, 667);
    opened.settle();
    assert_eq!(opened.builds(), 2, "rebuilt after the panic");
    assert_eq!(opened.readiness(), Readiness::Ready);
    assert_eq!(mirrored(&opened.collection), expected(&opened.collection));
    let states: Vec<Readiness> = opened.readiness_events().into_iter().map(|e| e.readiness).collect();
    assert!(states.contains(&Readiness::Stale), "{states:?}");
    // The derived writer still applies commits.
    set_due(&opened.collection, id, 5);
    opened.host.flush().unwrap();
    assert!(mirrored(&opened.collection).iter().any(|r| r.0 == id.0 && r.4 == 5));
    assert_eq!(mirrored(&opened.collection), expected(&opened.collection));
    assert_eq!((opened.builds(), opened.readiness()), (2, Readiness::Ready));
    opened.close();
}

#[test]
fn a_panicking_build_batch_fails_that_refresh_which_runs_again_and_the_writer_goes_on() {
    let (_dir, path) = copy_fixture("schema-18.anki2");
    without_store(&path, |collection| add_cards(collection, &(0..10).collect::<Vec<_>>()));
    let opened = open_with(&path, |counters| CardMirror {
        batch: 4,
        panic_in_first_batch: true,
        ..CardMirror::new(counters)
    });
    assert!(
        opened.host.wait_idle(Duration::from_secs(10)),
        "the derived store did not settle: {:?}",
        opened.readiness()
    );
    assert_eq!(opened.builds(), 2, "the failed refresh ran again");
    assert_eq!(opened.readiness(), Readiness::Ready);
    assert_eq!(mirrored(&opened.collection), expected(&opened.collection));
    let id = add_cards(&opened.collection, &[77])[0];
    opened.host.flush().unwrap();
    assert!(mirrored(&opened.collection).iter().any(|r| r.0 == id.0 && r.4 == 77));
    opened.close();
}

#[test]
fn a_build_that_goes_on_after_a_failed_batch_does_not_make_the_component_ready() {
    let (_dir, path) = copy_fixture("schema-18.anki2");
    without_store(&path, |collection| add_cards(collection, &(0..10).collect::<Vec<_>>()));
    let opened = open_with(&path, |counters| CardMirror {
        batch: 4,
        panic_in_first_batch: true,
        ignore_batch_errors: true,
        ..CardMirror::new(counters)
    });
    opened.settle();
    assert_eq!(opened.builds(), 2, "the refresh with a failed batch ran again");
    assert_eq!(opened.readiness(), Readiness::Ready);
    assert_eq!(mirrored(&opened.collection), expected(&opened.collection));
    opened.close();
}

#[test]
fn a_kept_delta_whose_apply_panics_at_the_swap_fails_that_refresh_which_runs_again() {
    let (_dir, path) = copy_fixture("schema-18.anki2");
    let ids = without_store(&path, |collection| add_cards(collection, &(0..10).collect::<Vec<_>>()));
    let gate = Arc::new(Gate::default());
    let opened = open_with(&path, |counters| CardMirror {
        batch: 2,
        gate: Some(Arc::clone(&gate)),
        panic_on_due: Some(667),
        ..CardMirror::new(counters)
    });
    opened.wait_for_first_batch();
    // Kept by the running build; its apply panics in the swap transaction.
    set_due(&opened.collection, ids[0], 667);
    opened.host.flush().unwrap();
    gate.open();
    assert!(
        opened.host.wait_idle(Duration::from_secs(10)),
        "the derived store did not settle: {:?}",
        opened.readiness()
    );
    assert_eq!(opened.builds(), 2, "the failed refresh ran again");
    assert_eq!(opened.readiness(), Readiness::Ready);
    assert_eq!(mirrored(&opened.collection), expected(&opened.collection));
    set_due(&opened.collection, ids[0], 5);
    opened.host.flush().unwrap();
    assert!(mirrored(&opened.collection).iter().any(|r| r.0 == ids[0].0 && r.4 == 5));
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
fn a_swap_that_fails_after_replacing_the_file_still_rebuilds_the_store_for_the_new_generation() {
    let (dir, path) = copy_fixture("schema-18.anki2");
    let replacement = replacement_with_cards(dir.path());
    let opened = open(&path);
    opened.settle();
    add_cards(&opened.collection, &[1_000, 1_001]);
    opened.host.flush().unwrap();
    let old = opened.collection.generation();
    let builds = opened.builds();

    let slot = opened.engine.acquire_slot(LongOp::Restore).unwrap();
    let mut exclusive = slot.enter_exclusive(false).unwrap();
    // The file is in place, then a later step of the swap fails
    // (`engine/lifecycle` L8 step 4): the collection was still replaced.
    let source = replacement.clone();
    let error = exclusive
        .replace(move |live| {
            fs::rename(&source, live).unwrap();
            Err(EngineError::Internal {
                message: "syncing the directory failed".to_owned(),
            })
        })
        .unwrap_err();
    assert!(matches!(error, EngineError::Internal { .. }), "{error:?}");
    let new = exclusive.collection().generation();
    assert_eq!(new, Generation(old.0 + 1));
    assert!(!opened.host.is_ready(MIRROR, new));
    assert_eq!(
        mirrored(exclusive.collection()),
        Vec::new(),
        "the old generation's rows are gone before the replace returns"
    );
    drop(exclusive);
    drop(slot);
    opened.settle();

    let current = opened.engine.collection().unwrap();
    assert_eq!(current.generation(), new);
    assert!(opened.host.is_ready(MIRROR, new));
    assert_eq!(opened.builds(), builds + 1);
    let rows = mirrored(&current);
    assert_eq!(rows, expected(&current));
    let dues: Vec<i64> = rows.iter().map(|r| r.4).collect();
    assert!(
        [2_000, 2_001, 2_002].iter().all(|due| dues.contains(due))
            && !dues.iter().any(|due| (1_000..2_000).contains(due)),
        "{dues:?}"
    );
    let after: Vec<Readiness> = opened
        .readiness_events()
        .into_iter()
        .filter(|e| e.generation == new)
        .map(|e| e.readiness)
        .collect();
    assert_eq!(after.first(), Some(&Readiness::Stale), "{after:?}");
    assert_eq!(after.last(), Some(&Readiness::Ready), "{after:?}");
    drop(current);
    opened.close();
}

#[test]
fn a_swap_that_fails_before_touching_the_file_keeps_the_store_bound_and_its_content() {
    let (_dir, path) = copy_fixture("schema-18.anki2");
    let opened = open(&path);
    opened.settle();
    add_cards(&opened.collection, &[1_000, 1_001]);
    opened.host.flush().unwrap();
    let before = mirrored(&opened.collection);
    let generation = opened.collection.generation();
    let builds = opened.builds();
    let _ = opened.readiness_events();

    let slot = opened.engine.acquire_slot(LongOp::Restore).unwrap();
    let mut exclusive = slot.enter_exclusive(false).unwrap();
    let error = exclusive
        .replace(|_| {
            Err(EngineError::Internal {
                message: "the staging copy failed its integrity check".to_owned(),
            })
        })
        .unwrap_err();
    assert!(matches!(error, EngineError::Internal { .. }), "{error:?}");
    assert_eq!(exclusive.collection().generation(), generation);
    drop(exclusive);
    drop(slot);
    opened.settle();

    // The readers the swap closed attach the store again when they reopen.
    assert!(opened.host.is_ready(MIRROR, generation));
    assert_eq!(mirrored(&opened.collection), before);
    assert_eq!(opened.builds(), builds, "an unchanged file needs no rebuild");
    assert_eq!(opened.readiness_events(), Vec::new());
    add_cards(&opened.collection, &[1_002]);
    opened.host.flush().unwrap();
    assert_eq!(mirrored(&opened.collection), expected(&opened.collection));
    opened.close();
}

#[test]
fn a_running_rebuild_yields_to_an_exclusive_phase_and_starts_again_after_it() {
    let (_dir, path) = copy_fixture("schema-18.anki2");
    // 50 batches of 30 ms: the rest of the first build takes well over the
    // second the exclusive job may wait for the maintenance reader below.
    without_store(&path, |collection| add_cards(collection, &(0..100).collect::<Vec<_>>()));
    let gate = Arc::new(Gate::default());
    let opened = open_with(&path, |counters| CardMirror {
        batch: 2,
        pause: Duration::from_millis(30),
        gate: Some(Arc::clone(&gate)),
        ..CardMirror::new(counters)
    });
    // The exclusive phase begins while the builder holds its snapshot,
    // stopped after its first batch; then the builder goes on.
    opened.wait_for_first_batch();
    let slot = opened.engine.acquire_slot(LongOp::Restore).unwrap();
    let exclusive = slot.enter_exclusive(true).unwrap();
    gate.open();
    // The exclusive job needs the maintenance reader (its backup): the build
    // lets go of it at its next batch.
    let started = monotonic::now();
    let lease = exclusive.collection().reader(ReaderRole::Maintenance).unwrap();
    assert!(
        monotonic::now() - started < Duration::from_secs(1),
        "the build held the maintenance reader for {:?}",
        monotonic::now() - started
    );
    drop(lease);
    assert_eq!(opened.builds(), 1, "a build started during the exclusive phase");
    // Cancelled without a replacement: the build starts again.
    drop(exclusive);
    drop(slot);
    opened.settle();
    assert_eq!(opened.builds(), 2, "the build that yielded did not run again");
    assert_eq!(opened.readiness(), Readiness::Ready);
    assert_eq!(mirrored(&opened.collection), expected(&opened.collection));
    opened.close();
}

#[test]
fn suspend_marks_the_store_clean_and_resume_marks_it_in_use() {
    let (_dir, path) = copy_fixture("schema-18.anki2");
    let opened = open(&path);
    opened.settle();
    assert_eq!(stamp_clean(&opened.collection), 0, "in use");
    let participant = opened.ports.suspend_participants[0].clone();
    participant.suspend().unwrap();
    assert_eq!(stamp_clean(&opened.collection), 1);
    participant.resume().unwrap();
    assert_eq!(stamp_clean(&opened.collection), 0);
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
fn a_rebuild_over_150k_cards_leaves_answers_fast_and_reports_its_progress_at_150k() {
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
