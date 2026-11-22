//! `stats/graphs-common` G1 to G4, G8 and G11, and `stats/browse-queries`
//! Q2, Q3 and Q6, on a schema-18 fixture copy opened by the production host.
//! The records are U99's and U101's: these tests read the resolved scope
//! every record is computed from. Names and values are made up.

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;
use std::sync::Arc;

use meguri_engine::{CancelToken, Collection, OpSpec};
use meguri_testkit::bench::fixtures::CoreHost;
use meguri_model::card::{Card, CardQueue, CardType};
use meguri_model::card_data::CardData;
use meguri_model::changes::OpSource;
use meguri_model::clock::{StepClock, UtcOffset};
use meguri_model::ids::{CardId, DeckId, NoteId, RevlogId};
use meguri_model::revlog::{RevlogEntry, RevlogKind};
use meguri_model::time::EpochSecs;
use meguri_model::usn::Usn;
use meguri_search::SearchMode;
use meguri_stats::graphs::{
    BrowseScope, Columns, GraphData, GraphRequest, GraphResult, History, Scope, ScopeError, Unreadable,
};
use meguri_storage::raw::SqlValue;
use meguri_storage::{Cell, ReaderRole, RowKey, StampMode, Table};
use tempfile::TempDir;

/// 2030-01-15T10:00:00Z.
pub const NOW_MS: i64 = 1_894_701_600_000;
/// The fixture's decks: `Lang`, its subdeck `Lang::Verbs`, the filtered
/// deck `Cram` (whose one card's home deck is `Lang`).
pub const LANG: i64 = 1_788_249_600_212;
pub const VERBS: i64 = 1_788_249_600_214;
pub const CRAM: i64 = 1_788_249_600_254;
/// A note of the fixture, for the cards the tests add.
pub const NOTE: i64 = 1_788_249_600_238;
/// The card in `Cram` whose home deck is `Lang`.
pub const CRAM_CARD: i64 = 1_788_249_600_233;

pub struct Fixture {
    _dir: TempDir,
    path_closed: std::path::PathBuf,
    pub clock: Arc<StepClock>,
    pub host: CoreHost,
}

pub fn fixture_path(dir: &Path) -> std::path::PathBuf {
    let collection = dir.join("collection").join("collection.anki2");
    fs::create_dir_all(collection.parent().unwrap()).unwrap();
    collection
}

impl Default for Fixture {
    fn default() -> Fixture {
        Fixture::new()
    }
}

impl Fixture {
    /// The fixture without its review-log rows.
    pub fn new() -> Fixture {
        Fixture::with_raw(&[])
    }

    /// [`Fixture::new`] after `statements` ran on the file before the host
    /// opened it: rows no op may write (an unreadable card fails the op's
    /// change set), as a scenario's `db_exec` writes them.
    pub fn with_raw(statements: &[&str]) -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let path = fixture_path(dir.path());
        fs::copy(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../conformance/fixtures/storage/schema-18.anki2"),
            &path,
        )
        .unwrap();
        if !statements.is_empty() {
            let clock = meguri_model::clock::FixedClock::new(NOW_MS, UtcOffset::UTC);
            let random = meguri_model::rng::SeededRandom::new(1);
            let opened = meguri_storage::open_collection(&path, &meguri_storage::OpenOptions::new(&clock, &random)).unwrap();
            for statement in statements {
                opened.writer().execute_raw(statement, &[]).unwrap();
            }
        }
        let clock = Arc::new(StepClock::new(NOW_MS, UtcOffset::UTC, "UTC"));
        let host = meguri_testkit::bench::fixtures::open_engine_collection(&path, clock.clone()).unwrap();
        let fixture = Fixture {
            _dir: dir,
            path_closed: path,
            clock,
            host,
        };
        let ids = fixture_revlog(&fixture.collection());
        fixture.write(move |scope| {
            for id in ids {
                scope.remove_revlog(RevlogId(id))?;
            }
            Ok(())
        });
        fixture
    }

    pub fn collection(&self) -> Collection {
        self.host.collection().unwrap()
    }

    /// Opens the collection again after its host was closed.
    pub fn reopen_closed(&mut self) {
        let path = self.path_closed.clone();
        self.host = meguri_testkit::bench::fixtures::open_engine_collection(&path, self.clock.clone()).unwrap();
    }

    /// Closes the host (the derived store persists and marks itself clean)
    /// and opens the collection again.
    pub fn reopen(&mut self) {
        let path = self.path();
        self.host.close().unwrap();
        self.host = meguri_testkit::bench::fixtures::open_engine_collection(&path, self.clock.clone()).unwrap();
    }

    pub fn path(&self) -> std::path::PathBuf {
        self.path_closed.clone()
    }

    /// Runs `work` in one op.
    pub fn write(&self, work: impl FnOnce(&mut meguri_storage::WriteScope<'_>) -> Result<(), meguri_storage::StorageError> + Send + 'static) {
        self.collection()
            .run(OpSpec::named("set_due_date", OpSource::UserUndoable), move |ctx| {
                work(ctx.scope())?;
                Ok(())
            })
            .unwrap();
    }

    /// Runs `work` in one op whose rows are written as given.
    pub fn write_raw(&self, work: impl FnOnce(&mut meguri_storage::WriteScope<'_>) -> Result<(), meguri_storage::StorageError> + Send + 'static) {
        self.collection()
            .run(
                OpSpec::named("sync_incoming", OpSource::SyncIncoming).stamp(StampMode::Raw),
                move |ctx| {
                    work(ctx.scope())?;
                    Ok(())
                },
            )
            .unwrap();
    }

    /// `N` as the day clock reports it.
    pub fn next_day_at(&self) -> i64 {
        self.collection().next_day_at().unwrap()
    }

    pub fn request(&self, scope: Scope, history: History) -> GraphResult {
        self.request_with(scope, history, Columns::Cached)
    }

    pub fn request_with(&self, scope: Scope, history: History, columns: Columns) -> GraphResult {
        let request = GraphRequest {
            scope,
            history,
            graphs: Vec::new(),
            refresh: 1,
            context: None,
            cancel: CancelToken::new(),
            columns,
        };
        self.host
            .stats()
            .request(&self.collection(), self.host.derived(), &request)
            .unwrap()
    }

    pub fn data(&self, scope: Scope, history: History) -> Box<GraphData> {
        match self.request(scope, history) {
            GraphResult::Data(data) => data,
            other => panic!("not data: {other:?}"),
        }
    }

    /// The scope's card ids and its rows (id, card), as the request reads
    /// them.
    pub fn read(&self, scope: Scope, history: History) -> (Vec<i64>, Vec<(i64, Option<i64>)>) {
        let data = self.data(scope, history);
        (
            data.scope.cards().map(|card| card.id).collect(),
            data.scope.rows().map(|row| (row.row.id, row.card)).collect(),
        )
    }

    pub fn rows(&self, scope: Scope, history: History) -> Vec<i64> {
        self.read(scope, history).1.into_iter().map(|(id, _)| id).collect()
    }

    pub fn cards(&self, scope: Scope, history: History) -> Vec<i64> {
        self.read(scope, history).0
    }

    /// Every table of the collection, row by row.
    pub fn dump(&self) -> Vec<Vec<Vec<SqlValue>>> {
        let collection = self.collection();
        let lease = collection.reader(ReaderRole::Query).unwrap();
        let snapshot = lease.snapshot().unwrap();
        ["col", "notes", "cards", "revlog", "decks", "deck_config", "config", "tags", "graves", "notetypes"]
            .iter()
            .map(|table| {
                snapshot
                    .execute_raw(&format!("SELECT * FROM {table} ORDER BY 1"), &[])
                    .unwrap()
                    .rows
            })
            .collect()
    }
}

fn fixture_revlog(collection: &Collection) -> Vec<i64> {
    let lease = collection.reader(ReaderRole::Query).unwrap();
    let snapshot = lease.snapshot().unwrap();
    snapshot
        .execute_raw("SELECT id FROM revlog", &[])
        .unwrap()
        .rows
        .into_iter()
        .map(|row| match row[0] {
            SqlValue::Integer(id) => id,
            ref other => panic!("{other:?}"),
        })
        .collect()
}

pub fn card(id: i64, deck: i64) -> Card {
    Card {
        id: CardId(id),
        note_id: NoteId(NOTE),
        deck_id: DeckId(deck),
        ord: 0,
        mtime: EpochSecs(0),
        usn: Usn(0),
        card_type: CardType::Review,
        queue: CardQueue::Review,
        due: 100,
        interval: 30,
        ease_factor: 2500,
        reps: 3,
        lapses: 0,
        remaining_steps: 0,
        original_due: 0,
        original_deck_id: DeckId(0),
        flags: 0,
        data: CardData::default(),
    }
}

pub fn revlog(id: i64, card: i64) -> RevlogEntry {
    RevlogEntry {
        id: RevlogId(id),
        card_id: CardId(card),
        usn: Usn(0),
        button: 3,
        interval: 30,
        last_interval: 10,
        ease_factor: 2500,
        answer_millis: 6_000,
        kind: RevlogKind::Review,
    }
}

/// A revlog row's cells, for raw values.
pub fn revlog_cells(id: i64, cid: i64, ease: Cell, last_ivl: Cell, factor: Cell, time: Cell, kind: Cell) -> Vec<Cell> {
    vec![
        Cell::Integer(id),
        Cell::Integer(cid),
        Cell::Integer(0),
        ease,
        Cell::Integer(1),
        last_ivl,
        factor,
        time,
        kind,
    ]
}

/// A raw `INSERT` of a card, for [`Fixture::with_raw`].
pub fn insert_card(id: i64, deck: i64, card_type: i64, queue: i64, due: i64) -> String {
    format!(
        "INSERT INTO cards (id, nid, did, ord, mod, usn, type, queue, due, ivl, factor, reps, lapses, left, \
         odue, odid, flags, data) VALUES ({id}, {NOTE}, {deck}, 0, 0, -1, {card_type}, {queue}, {due}, 3, 2500, \
         1, 0, 0, {due}, 0, 0, '{{}}')"
    )
}

fn add_deck(fixture: &Fixture, native: &str) -> i64 {
    let deck = meguri_engine::decks::new_deck(native.to_owned());
    let (tx, rx) = std::sync::mpsc::channel();
    fixture.write(move |scope| {
        tx.send(scope.add_deck(&deck)?.0).unwrap();
        Ok(())
    });
    rx.recv().unwrap()
}

const DAY_MS: i64 = 86_400_000;

#[test]
fn g2_deleted_card_rows_count_only_in_the_collection_scope() {
    let f = Fixture::new();
    let today = NOW_MS - 1_000;
    f.write(move |scope| {
        scope.put(&card(100, LANG))?;
        for at in 0..5 {
            scope.insert_revlog(&revlog(today - at, 100))?;
        }
        for at in 5..7 {
            scope.insert_revlog(&revlog(today - at, 999))?;
        }
        Ok(())
    });
    for history in [History::Last12Months, History::AllHistory] {
        assert_eq!(f.rows(Scope::Collection, history).len(), 7);
        assert_eq!(f.rows(Scope::Search("  \u{a0}".into()), history).len(), 7);
        assert_eq!(f.rows(Scope::Deck(DeckId(LANG)), history).len(), 5);
        assert_eq!(f.rows(Scope::Search("deck:*".into()), history).len(), 5);
    }
    let (_, rows) = f.read(Scope::Collection, History::AllHistory);
    assert_eq!(rows.iter().filter(|(_, card)| card.is_none()).count(), 2);
    assert!(rows.iter().filter(|(_, card)| card.is_some()).all(|(_, card)| *card == Some(100)));
}

#[test]
fn g2_a_moved_card_brings_its_rows() {
    let f = Fixture::new();
    let a = add_deck(&f, "Alpha");
    let b = add_deck(&f, "Beta");
    f.write(move |scope| {
        scope.put(&card(100, a))?;
        for at in 0..3 {
            scope.insert_revlog(&revlog(NOW_MS - 10 * DAY_MS - at, 100))?;
        }
        Ok(())
    });
    assert_eq!(f.rows(Scope::Deck(DeckId(a)), History::AllHistory).len(), 3);
    f.write(move |scope| {
        scope.put(&card(100, b))?;
        Ok(())
    });
    for history in [History::Last12Months, History::AllHistory] {
        assert_eq!(f.rows(Scope::Deck(DeckId(b)), history).len(), 3);
        assert_eq!(f.rows(Scope::Deck(DeckId(a)), history).len(), 0);
    }
}

#[test]
fn g2_1_a_filtered_card_is_in_its_deck_and_its_home_deck() {
    let f = Fixture::new();
    let lang = f.cards(Scope::Deck(DeckId(LANG)), History::AllHistory);
    assert!(lang.contains(&CRAM_CARD));
    assert_eq!(f.cards(Scope::Deck(DeckId(CRAM)), History::AllHistory), vec![CRAM_CARD]);
    let verbs = f.cards(Scope::Deck(DeckId(VERBS)), History::AllHistory);
    assert!(!verbs.contains(&CRAM_CARD));
    assert!(!verbs.is_empty());
    // Lang holds its subdeck's cards.
    assert!(verbs.iter().all(|card| lang.contains(card)));
    assert_eq!(f.cards(Scope::Deck(DeckId(1)), History::AllHistory), Vec::<i64>::new());
}

#[test]
fn g2_1_the_subtree_by_name_under_simple_folding_never_wildcards() {
    let f = Fixture::new();
    let ab = add_deck(&f, "a_b");
    let abc = add_deck(&f, "a_b\u{1f}c");
    let axb = add_deck(&f, "axb");
    let upper = add_deck(&f, "ÜBER");
    let lower_child = add_deck(&f, "über\u{1f}x");
    let strasse = add_deck(&f, "STRASSE");
    let strasse_child = add_deck(&f, "straße\u{1f}y");
    let decks = [ab, abc, axb, upper, lower_child, strasse, strasse_child];
    f.write(move |scope| {
        for (at, deck) in decks.iter().enumerate() {
            scope.put(&card(200 + at as i64, *deck))?;
        }
        Ok(())
    });
    let cards = |deck: i64| f.cards(Scope::Deck(DeckId(deck)), History::AllHistory);
    assert_eq!(cards(ab), vec![200, 201]);
    assert_eq!(cards(axb), vec![202]);
    assert_eq!(cards(upper), vec![203, 204]);
    // ß folds to itself, never to ss.
    assert_eq!(cards(strasse), vec![205]);
}

#[test]
fn g2_1_a_deck_that_names_no_deck_is_a_scope_error() {
    let f = Fixture::new();
    assert!(matches!(
        f.request(Scope::Deck(DeckId(424_242)), History::AllHistory),
        GraphResult::ScopeError(ScopeError::UnknownDeck)
    ));
}

#[test]
fn g2_a_search_that_fails_is_a_scope_error_with_no_data() {
    let f = Fixture::new();
    for text in ["prop:foo<", "prop:ivl>", "\"abc"] {
        match f.request(Scope::Search(text.into()), History::AllHistory) {
            GraphResult::ScopeError(ScopeError::Search(error)) => {
                assert_eq!(error, meguri_search::parse(text).unwrap_err(), "{text}")
            }
            other => panic!("{text}: {other:?}"),
        }
    }
}

#[test]
fn g2_a_search_follows_a_retag_between_requests() {
    let f = Fixture::new();
    f.write(move |scope| {
        scope.put(&card(100, LANG))?;
        scope.insert_revlog(&revlog(NOW_MS - 5_000, 100))?;
        Ok(())
    });
    let tagged = || f.rows(Scope::Search("tag:verbs".into()), History::AllHistory);
    assert!(tagged().is_empty());
    let collection = f.collection();
    let lease = collection.reader(ReaderRole::Query).unwrap();
    let mut note: meguri_model::note::Note = meguri_storage::ReadRows::note(&*lease.snapshot().unwrap(), NoteId(NOTE))
        .unwrap()
        .unwrap();
    drop(lease);
    note.tags = " verbs ".into();
    f.write(move |scope| {
        scope.put(&note)?;
        Ok(())
    });
    assert_eq!(tagged().len(), 1);
}

#[test]
fn g3_the_window_starts_366_days_before_n_and_never_reads_a_negative_id() {
    let f = Fixture::new();
    let n = f.next_day_at();
    let start = (n - 366 * 86_400) * 1000;
    let ids = [start - 1, start, start + 1, NOW_MS, (n + 86_400) * 1000, -5];
    f.write(move |scope| {
        scope.put(&card(100, LANG))?;
        for id in ids {
            scope.insert_revlog(&revlog(id, 100))?;
        }
        Ok(())
    });
    assert_eq!(
        f.rows(Scope::Collection, History::Last12Months),
        vec![start, start + 1, NOW_MS, (n + 86_400) * 1000]
    );
    assert_eq!(
        f.rows(Scope::Collection, History::AllHistory),
        vec![start - 1, start, start + 1, NOW_MS, (n + 86_400) * 1000]
    );
    // Card graphs read every card whatever the history.
    assert_eq!(
        f.cards(Scope::Collection, History::Last12Months),
        f.cards(Scope::Collection, History::AllHistory)
    );
}

#[test]
fn g4_odd_row_values_read_by_the_graph_rules() {
    let f = Fixture::new();
    let base = NOW_MS - 10_000;
    let rows: Vec<Vec<Cell>> = vec![
        revlog_cells(base, 100, Cell::Integer(3), Cell::Integer(5), Cell::Integer(0), Cell::Integer(10), Cell::Integer(260)),
        revlog_cells(base + 1, 100, Cell::Integer(3), Cell::Integer(5), Cell::Integer(0), Cell::Integer(-5), Cell::Integer(-1)),
        revlog_cells(base + 2, 100, Cell::Integer(3), Cell::Real(20.9), Cell::Integer(0), Cell::Integer(5_000_000_000), Cell::Integer(6)),
        revlog_cells(base + 3, 100, Cell::Integer(3), Cell::Integer(21), Cell::Integer(0), Cell::Real(1.5), Cell::Real(1.5)),
        // INTEGER affinity stores "9" as 9: a text that is no number stays
        // text.
        revlog_cells(base + 4, 100, Cell::Integer(3), Cell::Integer(-86_400), Cell::Integer(0), Cell::text("x9"), Cell::text("x1")),
    ];
    f.write_raw(move |scope| {
        scope.put(&card(100, LANG))?;
        for row in rows {
            scope.restore(Table::Revlog, &RowKey::Id(match row[0] {
                Cell::Integer(id) => id,
                _ => unreachable!(),
            }), Some(&row))?;
        }
        Ok(())
    });
    let data = f.data(Scope::Collection, History::AllHistory);
    let read: Vec<(RevlogKind, u32, i32)> = data.scope.rows().map(|r| (r.row.kind, r.row.time, r.row.last_ivl)).collect();
    assert_eq!(
        read,
        vec![
            (RevlogKind::Manual, 10, 5),
            (RevlogKind::Learning, 0, 5),
            (RevlogKind::Learning, 0, 20),
            (RevlogKind::Learning, 0, 21),
            (RevlogKind::Learning, 0, -86_400),
        ]
    );
}

#[test]
fn g4_an_unreadable_row_in_the_window_fails_the_request() {
    let f = Fixture::new();
    let old = NOW_MS - 400 * DAY_MS;
    for (column, cells) in [
        (
            "ease",
            revlog_cells(old, 100, Cell::Integer(300), Cell::Integer(1), Cell::Integer(0), Cell::Integer(1), Cell::Integer(1)),
        ),
        (
            "factor",
            revlog_cells(old, 100, Cell::Integer(3), Cell::Integer(1), Cell::Integer(-1), Cell::Integer(1), Cell::Integer(1)),
        ),
        (
            "lastIvl",
            revlog_cells(old, 100, Cell::Integer(3), Cell::Integer(3_000_000_000), Cell::Integer(0), Cell::Integer(1), Cell::Integer(1)),
        ),
    ] {
        f.write_raw(move |scope| {
            scope.put(&card(100, LANG))?;
            scope.restore(Table::Revlog, &RowKey::Id(old), Some(&cells))?;
            Ok(())
        });
        assert!(matches!(f.request(Scope::Collection, History::Last12Months), GraphResult::Data(_)));
        match f.request(Scope::Collection, History::AllHistory) {
            GraphResult::UnreadableRow(bad) => assert_eq!(
                bad,
                Unreadable {
                    table: "revlog",
                    column,
                    key: old
                }
            ),
            other => panic!("{column}: {other:?}"),
        }
        // A row of a card outside the scope changes nothing.
        assert!(matches!(f.request(Scope::Deck(DeckId(VERBS)), History::AllHistory), GraphResult::Data(_)));
    }
}

/// A card of `Lang::Verbs` (`a`) with raw `type`, `queue`, `due` and `odue`,
/// and a readable one in `Lang` (`b`), written before the host opens.
fn raw_cards(card_type: i64, queue: i64, due: i64) -> Fixture {
    Fixture::with_raw(&[&insert_card(300, VERBS, card_type, queue, due), &insert_card(301, CRAM, 2, 2, 5)])
}

#[test]
fn g4_an_unreadable_card_fails_its_scopes_whatever_the_graphs() {
    for (column, card_type, queue) in [("type", 7, 2), ("queue", 2, 9), ("type", 263, 2)] {
        let f = raw_cards(card_type, queue, 5);
        for history in [History::Last12Months, History::AllHistory] {
            for scope in [Scope::Deck(DeckId(VERBS)), Scope::Deck(DeckId(LANG)), Scope::Collection] {
                match f.request(scope.clone(), history) {
                    GraphResult::UnreadableRow(bad) => assert_eq!(
                        bad,
                        Unreadable {
                            table: "cards",
                            column,
                            key: 300
                        }
                    ),
                    other => panic!("{column} {scope:?}: {other:?}"),
                }
            }
            // A scope without the card reads.
            assert!(matches!(f.request(Scope::Deck(DeckId(CRAM)), history), GraphResult::Data(_)));
        }
    }
    // `type` 258 reads as 2 (the low 8 bits); a due beyond i32 reads 0 and
    // is readable.
    let f = raw_cards(258, 2, 1 << 40);
    let data = f.data(Scope::Deck(DeckId(VERBS)), History::AllHistory);
    let card = data.scope.cards().find(|card| card.id == 300).unwrap();
    assert_eq!((card.card_type, card.due, card.odue), (CardType::Review, 0, 0));
}

#[test]
fn g4_an_unreadable_review_row_of_another_scope_changes_nothing() {
    let f = Fixture::new();
    let id = NOW_MS - 1_000;
    f.write_raw(move |scope| {
        scope.put(&card(100, VERBS))?;
        let cells = revlog_cells(id, 100, Cell::Integer(300), Cell::Integer(1), Cell::Integer(0), Cell::Integer(1), Cell::Integer(1));
        scope.restore(Table::Revlog, &RowKey::Id(id), Some(&cells))?;
        Ok(())
    });
    assert!(matches!(f.request(Scope::Deck(DeckId(VERBS)), History::Last12Months), GraphResult::UnreadableRow(_)));
    assert!(matches!(f.request(Scope::Deck(DeckId(CRAM)), History::Last12Months), GraphResult::Data(_)));
    // The card's scope by search too.
    assert!(matches!(
        f.request(Scope::Search("deck:Lang::Verbs".into()), History::AllHistory),
        GraphResult::UnreadableRow(_)
    ));
}

#[test]
fn g1_the_context_and_g8_3_the_token() {
    let f = Fixture::new();
    let first = f.data(Scope::Collection, History::AllHistory);
    let context = first.context.clone();
    assert_eq!(context.now_ms, NOW_MS);
    assert_eq!(context.next_day_at, f.next_day_at());
    assert_eq!(context.rollover_hour, 4);
    assert_eq!(context.zone, "UTC");
    let second = |token, refresh, history| {
        let request = GraphRequest {
            scope: Scope::Collection,
            history,
            graphs: Vec::new(),
            refresh,
            context: Some(token),
            cancel: CancelToken::new(),
            columns: Columns::Cached,
        };
        f.host.stats().request(&f.collection(), f.host.derived(), &request).unwrap()
    };
    match second(first.token.clone(), 1, History::AllHistory) {
        GraphResult::Data(data) => assert_eq!(data.context, context),
        other => panic!("{other:?}"),
    }
    // Another refresh or history: Context changed.
    assert!(matches!(second(first.token.clone(), 2, History::AllHistory), GraphResult::ContextChanged));
    assert!(matches!(second(first.token.clone(), 1, History::Last12Months), GraphResult::ContextChanged));
    // A commit between the two requests: Context changed.
    f.clock.set_now_ms(NOW_MS + 60_000);
    f.write(|scope| {
        scope.put(&card(100, LANG))?;
        Ok(())
    });
    assert!(matches!(second(first.token.clone(), 1, History::AllHistory), GraphResult::ContextChanged));
}

#[test]
fn g8_a_cancelled_request_returns_cancelled() {
    let f = Fixture::new();
    let cancel = CancelToken::new();
    cancel.cancel();
    let request = GraphRequest {
        scope: Scope::Search("deck:*".into()),
        history: History::AllHistory,
        graphs: Vec::new(),
        refresh: 1,
        context: None,
        cancel,
        columns: Columns::Cached,
    };
    assert!(matches!(
        f.host.stats().request(&f.collection(), f.host.derived(), &request).unwrap(),
        GraphResult::Cancelled
    ));
}

#[test]
fn g8_a_graph_not_built_yet_is_an_error_not_made_up_values() {
    let f = Fixture::new();
    let request = GraphRequest {
        scope: Scope::Collection,
        history: History::AllHistory,
        graphs: vec![meguri_stats::graphs::Graph::Calendar],
        refresh: 1,
        context: None,
        cancel: CancelToken::new(),
        columns: Columns::Cached,
    };
    let result = f.host.stats().request(&f.collection(), f.host.derived(), &request);
    assert!(matches!(
        result,
        Err(meguri_stats::graphs::StatsError::NotBuilt(meguri_stats::graphs::Graph::Calendar))
    ));
}

#[test]
fn g7_preferences_are_read_with_their_defaults() {
    let f = Fixture::new();
    let read = || f.data(Scope::Collection, History::AllHistory).preferences;
    let defaults = read();
    assert_eq!(
        (
            defaults.first_day_of_week,
            defaults.card_counts_separate_inactive,
            defaults.future_due_show_backlog,
            defaults.fsrs
        ),
        (0, true, true, false)
    );
    use meguri_storage::ConfigValue;
    for (values, expected) in [
        ([ConfigValue::Integer(1), ConfigValue::Bool(false), ConfigValue::Bool(false), ConfigValue::Bool(true)], (1, false, false, true)),
        ([ConfigValue::Integer(5), ConfigValue::Integer(0), ConfigValue::String("false".into()), ConfigValue::Integer(1)], (5, true, true, false)),
        ([ConfigValue::Float(1.0), ConfigValue::Null, ConfigValue::Integer(1), ConfigValue::String("true".into())], (0, true, true, false)),
        ([ConfigValue::Integer(3), ConfigValue::Bool(true), ConfigValue::Bool(true), ConfigValue::Null], (0, true, true, false)),
    ] {
        f.write(move |scope| {
            for (key, value) in ["firstDayOfWeek", "cardCountsSeparateInactive", "futureDueShowBacklog", "fsrs"]
                .iter()
                .zip(values.iter())
            {
                scope.set_config(key, value)?;
            }
            Ok(())
        });
        let read = read();
        assert_eq!(
            (read.first_day_of_week, read.card_counts_separate_inactive, read.future_due_show_backlog, read.fsrs),
            expected
        );
    }
}

#[test]
fn g11_requests_write_nothing() {
    let f = Fixture::new();
    f.write(move |scope| {
        scope.put(&card(100, LANG))?;
        scope.insert_revlog(&revlog(NOW_MS - 3 * DAY_MS, 100))?;
        Ok(())
    });
    // The day clock's own check first (G1); then nothing changes.
    f.collection().check_rollover().unwrap();
    let before = f.dump();
    for history in [History::Last12Months, History::AllHistory] {
        for scope in [
            Scope::Collection,
            Scope::Deck(DeckId(LANG)),
            Scope::Deck(DeckId(CRAM)),
            Scope::Search("deck:current".into()),
            Scope::Search("prop:r>0.5 or is:due or rated:3".into()),
            Scope::Search("prop:foo<".into()),
        ] {
            let _ = f.request(scope, history);
        }
    }
    assert_eq!(f.dump(), before);
}

#[test]
fn q2_the_browse_scope_text_of_each_scope() {
    let f = Fixture::new();
    assert_eq!(f.data(Scope::Collection, History::AllHistory).browse_scope, BrowseScope::None);
    assert_eq!(f.data(Scope::Search(" \u{3000} ".into()), History::AllHistory).browse_scope, BrowseScope::None);
    assert_eq!(
        f.data(Scope::Deck(DeckId(LANG)), History::AllHistory).browse_scope,
        BrowseScope::Text("deck:\"Lang\"".into())
    );
    assert_eq!(
        f.data(Scope::Deck(DeckId(VERBS)), History::AllHistory).browse_scope,
        BrowseScope::Text("deck:\"Lang::Verbs\"".into())
    );
    assert_eq!(
        f.data(Scope::Search("  tag:a or tag:b ".into()), History::AllHistory).browse_scope,
        BrowseScope::Text("tag:a or tag:b".into())
    );
    let odd = add_deck(&f, "a\u{a0}b");
    let child = add_deck(&f, "a\u{a0}b\u{1f}c");
    let mut ids = [odd, child];
    ids.sort_unstable();
    assert_eq!(
        f.data(Scope::Deck(DeckId(odd)), History::AllHistory).browse_scope,
        BrowseScope::Text(format!("did:{},{}", ids[0], ids[1]))
    );
}

/// Q6's test-only join, built from `stats_browse_scope`'s text: Q3. AE60's
/// test (`graphs_cards`) uses it too.
pub fn join(scope: &BrowseScope, item: &str) -> Option<String> {
    // Q3: an item with an OR at its top level (outside every group and
    // every quoted text) is grouped.
    let top_level_or = {
        let (mut depth, mut quoted, mut escaped, mut found) = (0i32, false, false, false);
        let chars: Vec<char> = item.chars().collect();
        for (at, c) in chars.iter().enumerate() {
            match c {
                _ if escaped => escaped = false,
                '\\' => escaped = true,
                '"' => quoted = !quoted,
                '(' if !quoted => depth += 1,
                ')' if !quoted => depth -= 1,
                ' ' if !quoted && depth == 0 => {
                    let next: String = chars[at + 1..].iter().take(3).collect();
                    found |= next.eq_ignore_ascii_case("or ");
                }
                _ => {}
            }
        }
        found
    };
    match scope {
        BrowseScope::None => Some(item.to_owned()),
        BrowseScope::Unavailable => None,
        BrowseScope::Text(scope) => Some(if top_level_or {
            format!("({scope}) AND ({item})")
        } else {
            format!("({scope}) AND {item}")
        }),
    }
}

fn put_deck(f: &Fixture, id: i64, native: &str) {
    let mut deck = meguri_engine::decks::new_deck(native.to_owned());
    deck.id = DeckId(id);
    f.write(move |scope| {
        scope.put(&deck)?;
        Ok(())
    });
}

#[test]
fn q3_the_join_table_byte_for_byte() {
    let f = Fixture::new();
    put_deck(&f, 1_700_000_000_101, "Japanese");
    put_deck(&f, 1_700_000_000_102, "a_b*");
    put_deck(&f, 1_700_000_000_103, "x \"y\"");
    put_deck(&f, 1_700_000_000_104, "a\\b\u{1f}c");
    put_deck(&f, 1_700_000_000_105, "Lang\u{1f}日本語");
    put_deck(&f, 1_700_000_000_106, "current");
    put_deck(&f, 1_700_000_000_107, "current\u{1f}a");
    put_deck(&f, 1_700_000_000_001, "a\u{a0}b");
    put_deck(&f, 1_700_000_000_002, "a\u{a0}b\u{1f}c");
    let young = "(\"is:review\" AND -\"is:learn\") AND \"prop:ivl<21\" AND -(\"is:buried\" OR \"is:suspended\")";
    let table: Vec<(Scope, &str, Option<String>)> = vec![
        (Scope::Collection, "\"is:new\"", Some("\"is:new\"".into())),
        (Scope::Collection, "\"is:new\" OR \"is:due\"", Some("\"is:new\" OR \"is:due\"".into())),
        (
            Scope::Deck(DeckId(LANG)),
            "\"is:new\" OR \"is:due\"",
            Some("(deck:\"Lang\") AND (\"is:new\" OR \"is:due\")".into()),
        ),
        (Scope::Deck(DeckId(1_700_000_000_101)), young, Some(format!("(deck:\"Japanese\") AND {young}"))),
        (Scope::Deck(DeckId(1_700_000_000_102)), "\"is:new\"", Some("(deck:\"a\\_b\\*\") AND \"is:new\"".into())),
        (
            Scope::Deck(DeckId(1_700_000_000_103)),
            "\"is:buried\"",
            Some("(deck:\"x \\\"y\\\"\") AND \"is:buried\"".into()),
        ),
        (
            Scope::Deck(DeckId(1_700_000_000_104)),
            "\"is:new\"",
            Some("(deck:\"a\\\\b::c\") AND \"is:new\"".into()),
        ),
        (
            Scope::Deck(DeckId(1_700_000_000_105)),
            "\"prop:rated=-3\"",
            Some("(deck:\"Lang::日本語\") AND \"prop:rated=-3\"".into()),
        ),
        (
            Scope::Deck(DeckId(1_700_000_000_106)),
            "\"is:suspended\"",
            Some("(deck:\"Current\") AND \"is:suspended\"".into()),
        ),
        (
            Scope::Deck(DeckId(1_700_000_000_001)),
            "\"is:suspended\"",
            Some("(did:1700000000001,1700000000002) AND \"is:suspended\"".into()),
        ),
        (
            Scope::Search("tag:a or tag:b".into()),
            "\"is:buried\"",
            Some("(tag:a or tag:b) AND \"is:buried\"".into()),
        ),
        (Scope::Search("  is:due  ".into()), "\"is:new\"", Some("(is:due) AND \"is:new\"".into())),
        (Scope::Search("   ".into()), "\"is:new\"", Some("\"is:new\"".into())),
    ];
    for (scope, item, expected) in table {
        let browse = f.data(scope.clone(), History::AllHistory).browse_scope;
        assert_eq!(join(&browse, item), expected, "{scope:?} {item}");
    }
    // The last row: the ids form with a negative id is unavailable.
    let g = Fixture::new();
    put_deck(&g, -5, "a\u{a0}b");
    let browse = g.data(Scope::Deck(DeckId(-5)), History::AllHistory).browse_scope;
    assert_eq!(browse, BrowseScope::Unavailable);
    assert_eq!(join(&browse, "\"is:new\""), None);
}

#[test]
fn r1_a_review_usn_beyond_32_bits_reads_and_one_of_another_class_does_not() {
    let f = Fixture::new();
    let id = NOW_MS - 2_000;
    f.write_raw(move |scope| {
        scope.put(&card(100, LANG))?;
        let mut cells = revlog_cells(id, 100, Cell::Integer(3), Cell::Integer(1), Cell::Integer(0), Cell::Integer(1), Cell::Integer(1));
        cells[2] = Cell::Integer(4_294_967_297);
        scope.restore(Table::Revlog, &RowKey::Id(id), Some(&cells))?;
        let mut text = revlog_cells(id + 1, 100, Cell::Integer(3), Cell::Integer(1), Cell::Integer(0), Cell::Integer(1), Cell::Integer(1));
        text[2] = Cell::text("x");
        scope.restore(Table::Revlog, &RowKey::Id(id + 1), Some(&text))?;
        Ok(())
    });
    match f.request(Scope::Collection, History::AllHistory) {
        GraphResult::UnreadableRow(bad) => assert_eq!(
            bad,
            Unreadable {
                table: "revlog",
                column: "usn",
                key: id + 1
            }
        ),
        other => panic!("{other:?}"),
    }
    f.write(move |scope| {
        scope.remove_revlog(RevlogId(id + 1))?;
        Ok(())
    });
    assert_eq!(f.rows(Scope::Collection, History::AllHistory), vec![id]);
}

#[test]
fn q6_joined_texts_parse_round_trip_and_find_the_scope_cards() {
    // `Filtered` and `filtered` are one name to the decks table: two runs.
    q6_for(&[
        "a_b",
        "a*b",
        "a\"b",
        "a\\b",
        "(x)",
        "-x",
        "x\u{1f}current",
        "Filtered",
        "current",
        "日本語\u{1f}子",
        "a b",
        "a\u{a0}b",
        "empty\u{1f}",
        "two::colons",
    ]);
    q6_for(&["filtered"]);
}

fn q6_for(names: &[&str]) {
    let f = Fixture::new();
    let mut decks = Vec::new();
    for (at, name) in names.iter().enumerate() {
        let deck = add_deck(&f, name);
        decks.push(deck);
        let new = at % 2 == 0;
        f.write(move |scope| {
            let mut first = card(500 + 2 * at as i64, deck);
            if new {
                first.card_type = CardType::New;
                first.queue = CardQueue::New;
            }
            scope.put(&first)?;
            scope.put(&card(501 + 2 * at as i64, deck))?;
            Ok(())
        });
    }
    let items = ["\"is:new\"", "\"is:review\" -\"is:suspended\"", "\"is:new\" OR \"is:due\""];
    let scopes: Vec<Scope> = decks
        .iter()
        .map(|deck| Scope::Deck(DeckId(*deck)))
        .chain([Scope::Collection, Scope::Search("tag:none or deck:Lang".into())])
        .collect();
    for scope in scopes {
        let data = f.data(scope.clone(), History::AllHistory);
        let scope_cards: BTreeSet<i64> = data.scope.cards().map(|card| card.id).collect();
        for item in items {
            let Some(text) = join(&data.browse_scope, item) else {
                panic!("{scope:?}: no joined text");
            };
            meguri_search::parse(&text).unwrap_or_else(|e| panic!("{text}: {e:?}"));
            let canonical = meguri_search::canonical_text(&text).unwrap();
            assert_eq!(meguri_search::canonical_text(&canonical).unwrap(), canonical, "{text}");
            let found: BTreeSet<i64> = meguri_engine::browse::search::search_ids(f.host.engine(), &text, SearchMode::Cards)
                .unwrap()
                .into_iter()
                .collect();
            let item_cards: BTreeSet<i64> = meguri_engine::browse::search::search_ids(f.host.engine(), item, SearchMode::Cards)
                .unwrap()
                .into_iter()
                .collect();
            let expected: BTreeSet<i64> = scope_cards.intersection(&item_cards).copied().collect();
            assert_eq!(found, expected, "{scope:?} {text}");
        }
    }
}
