//! Browse's filter counts (`specs/search/filter-tokens.md` T12.4, T16's
//! last row): every count equals the number of rows the same token shows
//! in a search session in that mode, Cards and Notes, on a collection with
//! a note without cards, a card without a note, a deck with subdecks and a
//! filtered deck, hierarchical tags, suspended and buried cards, flags and
//! review history; the built-in saved searches, a user saved search and the
//! deck page's Suspended Cards row (plan decision 18). Then the cache: a
//! count is read again only after a change set touches a table it read, a
//! newer call or the caller's token cancels, and a halt refuses.

mod common;

use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;

use common::Harness;
use common::tagging::raw;
use meguri_engine::browse::counts::deck_suspended;
use meguri_engine::browse::search::{BrowseError, SearchEvent, SearchListener, SearchSession, SortDirection, SortSpec};
use meguri_engine::browse::{FilterCounts, NameIndexProvider};
use meguri_engine::sched::ops::{self, Request, Targets};
use meguri_engine::{CancelToken, EngineError, Ports, browser_config};
use meguri_model::ids::{CardId, DeckId};
use meguri_search::SearchMode;
use meguri_search::filter_tokens::counts::CountItem;
use meguri_search::filter_tokens::suggest::BuiltIn;
use meguri_search::filter_tokens::{
    DeckValue, DueValue, FilterQuery, NotetypeValue, Period, StateValue, Token, Value, ValueToken,
};
use meguri_search::name_index::NameIndex;
use meguri_storage::raw::SqlValue;

const WAIT: Duration = Duration::from_secs(30);

/// The fixture's note types: Pair (Front, Back; Forward, Reverse) and Gap
/// (Text, Extra; Gap).
const PAIR: i64 = 1_788_249_600_186;
const GAP: i64 = 1_788_249_600_189;
const LANG: i64 = 1_788_249_600_212;
const VERBS: i64 = 1_788_249_600_214;
const CRAM: i64 = 1_788_249_600_254;
const ZANDOR: i64 = 300;
const OTHER: i64 = 301;
const DAY_MS: i64 = 86_400_000;

struct Fx {
    h: Harness,
    names: Arc<NameIndexProvider>,
    counts: Arc<FilterCounts>,
}

fn int(v: i64) -> SqlValue {
    SqlValue::Integer(v)
}

fn text(v: &str) -> SqlValue {
    SqlValue::Text(v.to_owned())
}

/// The schema-18 fixture with its notes, cards, review log and tags
/// replaced by this file's rows.
fn fixture() -> Fx {
    let names = Arc::new(NameIndexProvider::new());
    let counts = Arc::new(FilterCounts::new());
    let ports = Ports::new()
        .open_step(names.clone())
        .observer(names.clone())
        .generation_listener(names.clone())
        .open_step(counts.clone())
        .observer(counts.clone())
        .generation_listener(counts.clone());
    let h = Harness::open_with(ports);
    let c = &h.collection;
    for table in ["notes", "cards", "revlog", "graves", "tags"] {
        raw(c, &format!("DELETE FROM {table}"), &[]);
    }
    // Two more decks: a third level under Lang::Verbs, and Other.
    for (id, name) in [(ZANDOR, "Lang\u{1f}Verbs\u{1f}Zandor"), (OTHER, "Other")] {
        raw(
            c,
            "INSERT INTO decks (id, name, mtime_secs, usn, common, kind) \
             SELECT ?1, ?2, mtime_secs, usn, common, kind FROM decks WHERE id = ?3",
            &[int(id), text(name), int(VERBS)],
        );
    }
    let now = common::NOW_MS;
    let today = i64::from(c.today().unwrap());
    // (id, note type, tags, days since edited)
    let notes: [(i64, i64, &str, i64); 7] = [
        (1, PAIR, " vocab vocab::verbs ", 0),
        (2, PAIR, " vocab::verbs::zandor Mixed_Case ", 3),
        (3, PAIR, " other ", 40),
        (4, PAIR, "", 10),
        (5, GAP, " leech VOCAB ", 1),
        // No cards: Notes mode shows it for note terms only.
        (6, PAIR, " vocab ", 0),
        (7, PAIR, " Vocabulary ", 0),
    ];
    for (id, mid, tags, edited) in notes {
        raw(
            c,
            "INSERT INTO notes (id,guid,mid,mod,usn,tags,flds,sfld,csum,flags,data) \
             VALUES (?1,?1,?2,?3,0,?4,'front\u{1f}back','front',0,0,'')",
            &[int(id), int(mid), int(now / 1000 - edited * 86_400), text(tags)],
        );
    }
    let cards: [CardRow; 10] = [
        (1, 1, LANG, 0, 0, 0, 0, 1, 0, 0),
        (2, 1, VERBS, 0, 1, 2, 2, today, 1, 3),
        (3, 2, ZANDOR, 0, 0, 2, -1, today + 3, 0, 20),
        (4, 2, 1, 0, 1, 0, -2, 2, 2, 20),
        // In the filtered deck, home Lang::Verbs, learning.
        (5, 3, CRAM, VERBS, 0, 1, 1, now / 1000 - 60, 0, 40),
        (6, 4, 1, 0, 0, 2, 2, today - 2, 7, 6),
        (7, 5, OTHER, 0, 0, 2, -1, today + 9, 0, 1),
        (8, 5, LANG, 0, 1, 0, 0, 3, 0, 1),
        // A card whose note is missing: Cards mode with card scope only.
        (9, 999, LANG, 0, 0, 0, 0, 4, 3, 0),
        (10, 7, VERBS, 0, 0, 2, -3, today + 1, 3, 100),
    ];
    for (k, nid, did, odid, ord, kind, queue, due, flags, added) in cards {
        raw(
            c,
            "INSERT INTO cards (id,nid,did,ord,mod,usn,type,queue,due,ivl,factor,reps,lapses,left,odue,odid,flags,data) \
             VALUES (?1,?2,?3,?4,0,0,?5,?6,?7,1,2500,1,0,0,?8,?9,?10,'')",
            &[
                int(card_id(k, added)),
                int(nid),
                int(did),
                int(ord),
                int(kind),
                int(queue),
                int(due),
                int(if odid == 0 { 0 } else { today }),
                int(odid),
                int(flags),
            ],
        );
    }
    // (card, days ago)
    for (k, (card, ago)) in [(2, 0), (6, 3), (3, 12)].into_iter().enumerate() {
        let added = cards.iter().find(|row| row.0 == card).map_or(0, |row| row.9);
        raw(
            c,
            "INSERT INTO revlog (id,cid,usn,ease,ivl,lastIvl,factor,time,type) VALUES (?1,?2,0,3,1,0,2500,1000,1)",
            &[int(now - ago * DAY_MS - 1000 + k as i64), int(card_id(card, added))],
        );
    }
    for tag in ["vocab", "vocab::verbs", "vocab::verbs::zandor", "Mixed_Case", "other", "leech", "Vocabulary"] {
        raw(c, "INSERT INTO tags (tag, usn, collapsed, config) VALUES (?1, 0, 0, NULL)", &[text(tag)]);
    }
    raw(
        c,
        "INSERT OR REPLACE INTO config (key, usn, mtime_secs, val) VALUES ('savedFilters', 0, 0, ?1)",
        &[SqlValue::Blob(br#"{"Verbs due":"deck:Lang::Verbs is:review","Every deck":"deck:*"}"#.to_vec())],
    );
    // Raw writes bring no change set: the index is built again here.
    names.rebuild(c).unwrap();
    Fx { h, names, counts }
}

/// (card, note, deck, home deck, ord, type, queue, due, flags, days since
/// added)
type CardRow = (i64, i64, i64, i64, i64, i64, i64, i64, i64, i64);

/// A card id is its creation time: `added` days before now, and its number.
fn card_id(k: i64, added: i64) -> i64 {
    common::NOW_MS - added * DAY_MS - 10_000 + k
}

struct Events(Mutex<mpsc::Sender<SearchEvent>>);

impl SearchListener for Events {
    fn event(&self, event: SearchEvent) {
        let _ = self.0.lock().unwrap().send(event);
    }
}

impl Fx {
    fn index(&self) -> Arc<NameIndex> {
        self.names.index()
    }

    fn count(&self, mode: SearchMode, items: &[CountItem]) -> Vec<Option<u64>> {
        self.counts
            .filter_counts(&self.h.engine, &self.index(), mode, items, None)
            .unwrap()
    }

    /// The rows a search session shows for `text` in `mode`.
    fn shown(&self, session: &SearchSession, events: &mpsc::Receiver<SearchEvent>, text: &str, mode: SearchMode) -> u64 {
        let key = match mode {
            SearchMode::Cards => "cardDue",
            SearchMode::Notes => "noteCrt",
        };
        let seq = session.start_search(text, mode, SortSpec::new(key, SortDirection::Ascending), None);
        loop {
            let event = events.recv_timeout(WAIT).expect("a search event");
            if event.seq() != seq || !event.is_terminal() {
                continue;
            }
            match event {
                SearchEvent::Complete { count, .. } => return count,
                other => panic!("{text:?} in {mode:?}: {other:?}"),
            }
        }
    }
}

/// Every value the sidebar and the suggestions count, with saved searches
/// and the Suspended Cards row.
fn items(names: &NameIndex) -> Vec<CountItem> {
    let mut values: Vec<Value> = vec![Value::CurrentDeck, Value::Untagged, Value::NoFlag, Value::AnyFlag];
    values.extend(names.decks().iter().map(|d| Value::Deck(DeckValue::of(names, d.id).unwrap())));
    values.extend(names.tags().iter().map(|t| Value::Tag(t.clone())));
    // An implicit parent and a tag no note holds.
    values.extend([Value::Tag("VOCAB::VERBS".to_owned()), Value::Tag("absent".to_owned())]);
    values.extend(names.notetypes().iter().map(|n| Value::Notetype(NotetypeValue::of(n))));
    values.extend(["Forward", "Reverse", "Gap"].map(|n| Value::CardType(n.to_owned())));
    values.extend(
        [
            StateValue::New,
            StateValue::Learning,
            StateValue::Review,
            StateValue::Suspended,
            StateValue::Buried,
            StateValue::Leech,
        ]
        .map(Value::State),
    );
    values.extend((1..=7).map(Value::Flag));
    values.extend([DueValue::Today, DueValue::Overdue, DueValue::Next7Days].map(Value::Due));
    for period in [Period::Today, Period::Last7Days, Period::Last30Days] {
        values.extend([Value::Added(period), Value::Edited(period), Value::Studied(period)]);
    }
    values.extend(BuiltIn::ALL.map(BuiltIn::value));
    let mut items: Vec<CountItem> = values.into_iter().map(CountItem::Value).collect();
    items.extend(names.saved_searches().iter().map(|s| CountItem::Saved(s.text.clone())));
    items.push(deck_suspended(names, DeckId(LANG)).unwrap());
    items.push(CountItem::Query(FilterQuery::new(
        vec![Token::Value(ValueToken::new(vec![Value::Tag("vocab".to_owned())], true))],
        "front",
    )));
    items
}

#[test]
fn every_count_equals_the_rows_its_search_shows() {
    let fx = fixture();
    let names = fx.index();
    assert!(names.decks().iter().any(|d| d.id == DeckId(ZANDOR)));
    let items = items(&names);
    let (sender, events) = mpsc::channel();
    let session = fx.h.engine.open_search_session(Arc::new(Events(Mutex::new(sender))));
    for mode in [SearchMode::Cards, SearchMode::Notes] {
        let counts = fx.count(mode, &items);
        assert_eq!(counts.len(), items.len());
        for (item, count) in items.iter().zip(&counts) {
            let run = item.run_text(&names).unwrap();
            let shown = fx.shown(&session, &events, &run, mode);
            assert_eq!(*count, Some(shown), "{run:?} in {mode:?}");
        }
    }
    // A few counts by hand, so that equal counts are not empty ones.
    let lang = Value::Deck(DeckValue::of(&names, DeckId(LANG)).unwrap());
    let vocab = Value::Tag("vocab".to_owned());
    let check = [
        // Lang: cards 1, 8 and 9 (no note) in it, 2 and 10 in Verbs, 3
        // under it, 5 through its home deck.
        (lang.clone(), SearchMode::Cards, 7),
        // Notes 1, 2, 3, 5 and 7; card 9's note is missing.
        (lang, SearchMode::Notes, 5),
        // vocab and its descendants, with letter case folded: notes 1, 2, 5
        // and 6 (no cards); Vocabulary is another tag.
        (vocab.clone(), SearchMode::Notes, 4),
        // Their cards: two each of notes 1, 2 and 5.
        (vocab, SearchMode::Cards, 6),
        (Value::State(StateValue::Suspended), SearchMode::Cards, 2),
        (Value::State(StateValue::Buried), SearchMode::Cards, 2),
        (Value::Flag(3), SearchMode::Cards, 2),
        (Value::Flag(3), SearchMode::Notes, 1),
        (Value::Untagged, SearchMode::Notes, 1),
    ];
    for (value, mode, expected) in check {
        assert_eq!(fx.count(mode, &[CountItem::Value(value.clone())]), [Some(expected)], "{value:?} {mode:?}");
    }
    // Plan decision 18: Lang's suspended cards (card 3, under Verbs).
    assert_eq!(fx.count(SearchMode::Cards, &[deck_suspended(&names, DeckId(LANG)).unwrap()]), [Some(1)]);
}

// A value T3 does not offer, and a saved search whose text fails when it
// runs, have no count; the others in the same call do.
#[test]
fn what_cannot_be_counted_is_none() {
    let fx = fixture();
    let items = [
        CountItem::Value(Value::Notetype(meguri_search::filter_tokens::NotetypeValue {
            id: meguri_model::ids::NotetypeId(-5),
            name: String::new(),
        })),
        CountItem::Saved("a -and b".to_owned()),
        CountItem::Value(Value::State(StateValue::New)),
    ];
    // New: cards 1, 4, 8 and 9 (its note missing).
    assert_eq!(fx.count(SearchMode::Cards, &items), [None, None, Some(4)]);
}

// T12.4's cache: a second call reads nothing; a config change keeps every
// count; a card change reads the card counts again and keeps a tag count
// of Notes mode, which reads no card.
#[test]
fn counts_are_cached_until_a_change_set_touches_what_they_read() {
    let fx = fixture();
    let names = fx.index();
    let suspended = CountItem::Value(Value::State(StateValue::Suspended));
    let lang = CountItem::Value(Value::Deck(DeckValue::of(&names, DeckId(LANG)).unwrap()));
    let vocab = CountItem::Value(Value::Tag("vocab".to_owned()));
    let all = [suspended, lang, vocab];
    let first = fx.count(SearchMode::Notes, &all);
    // Notes 2 and 5 have a suspended card; Lang holds five notes; vocab four.
    assert_eq!(first, [Some(2), Some(5), Some(4)]);
    let ran = fx.counts.statements_for_testing();
    assert_eq!(ran, 3, "one statement per kind");
    assert_eq!(fx.count(SearchMode::Notes, &all), first);
    assert_eq!(fx.counts.statements_for_testing(), ran, "cached");

    // A saved search is config only.
    browser_config::save_search(&fx.h.collection, "Later", "is:new").unwrap();
    assert_eq!(fx.count(SearchMode::Notes, &all), first);
    assert_eq!(fx.counts.statements_for_testing(), ran, "a config change keeps the counts");

    // Suspend card 1 (note 1, in Lang): the state and deck counts are
    // read again, with the notes that exist (Q5); the tag count of Notes
    // mode, which reads notes only, is not.
    ops::run(
        &fx.h.collection,
        Request::Suspend(Targets::Cards(vec![CardId(card_id(1, 0))])),
        &Default::default(),
    )
    .unwrap();
    assert_eq!(fx.count(SearchMode::Notes, &all), [Some(3), Some(5), Some(4)]);
    assert_eq!(fx.counts.statements_for_testing(), ran + 3);
    // Cards mode keeps its own entries.
    let cards = fx.count(SearchMode::Cards, &all);
    let ran = fx.counts.statements_for_testing();
    assert_eq!(fx.count(SearchMode::Cards, &all), cards);
    assert_eq!(fx.counts.statements_for_testing(), ran);
}

// A count of a value that reads the clock is read on every call.
#[test]
fn counts_that_read_now_are_read_again() {
    let fx = fixture();
    let overdue = [CountItem::Value(Value::Due(DueValue::Overdue))];
    fx.count(SearchMode::Cards, &overdue);
    let ran = fx.counts.statements_for_testing();
    fx.h.clock.step_ms(1000);
    fx.count(SearchMode::Cards, &overdue);
    assert_eq!(fx.counts.statements_for_testing(), ran + 1);
}

#[test]
fn a_cancelled_call_returns_cancelled_and_a_halt_refuses() {
    let fx = fixture();
    let token = CancelToken::new();
    token.cancel();
    let names = fx.index();
    let items = [CountItem::Value(Value::State(StateValue::New))];
    let result = fx
        .counts
        .filter_counts(&fx.h.engine, &names, SearchMode::Cards, &items, Some(&token));
    assert!(matches!(result, Err(BrowseError::Engine(EngineError::Cancelled))), "{result:?}");
    let refused = SearchSession::while_searches_stopped_for_testing(&fx.h.engine, || {
        fx.counts.filter_counts(&fx.h.engine, &names, SearchMode::Cards, &items, None)
    });
    assert!(matches!(refused, Err(BrowseError::Engine(_))), "{refused:?}");
    // The provider still counts afterwards.
    assert_eq!(fx.count(SearchMode::Cards, &items), [Some(4)]);
}

// Without its open step the provider keeps nothing, and still counts.
#[test]
fn an_unattached_provider_counts_without_a_cache() {
    let fx = fixture();
    let loose = FilterCounts::new();
    let names = fx.index();
    let items = [CountItem::Value(Value::State(StateValue::New))];
    for _ in 0..2 {
        let counts = loose.filter_counts(&fx.h.engine, &names, SearchMode::Cards, &items, None).unwrap();
        assert_eq!(counts, [Some(4)]);
    }
    assert_eq!(loose.statements_for_testing(), 2);
}

/// Every count the sidebar shows (`browser/workspace` V6): the built-in
/// saved searches, states, flags, every deck, note type and tag.
fn sidebar(names: &NameIndex) -> Vec<CountItem> {
    let mut values: Vec<Value> = BuiltIn::ALL.map(BuiltIn::value).to_vec();
    values.extend(
        [
            StateValue::New,
            StateValue::Learning,
            StateValue::Review,
            StateValue::Suspended,
            StateValue::Buried,
        ]
        .map(Value::State),
    );
    values.push(Value::NoFlag);
    values.extend((1..=7).map(Value::Flag));
    values.extend(names.decks().iter().filter_map(|d| DeckValue::of(names, d.id).map(Value::Deck)));
    values.extend(names.notetypes().iter().map(|n| Value::Notetype(NotetypeValue::of(n))));
    values.extend(names.tags().iter().map(|t| Value::Tag(t.clone())));
    values.into_iter().map(CountItem::Value).collect()
}

// T16's last row: on the 150k collection every count of the sidebar equals
// the number of ids `search_ids` returns for its run text. Its 200 ms
// budget is the `browse_filters` bench's (`tools/mg bench`).
#[test]
fn sidebar_counts_equal_search_ids_at_150k() {
    let generated = meguri_testkit::bench::fixtures::profile_collection("150k").expect("the 150k collection");
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("collection.anki2");
    std::fs::copy(generated.dir.join("collection.anki2"), &path).unwrap();
    let h = Harness::open_path(dir, path);
    let provider = NameIndexProvider::new();
    provider.rebuild(&h.collection).unwrap();
    let names = provider.index();
    let items = sidebar(&names);
    assert!(items.len() > 1000, "{} counts", items.len());
    for mode in [SearchMode::Cards, SearchMode::Notes] {
        let counts = FilterCounts::new().filter_counts(&h.engine, &names, mode, &items, None).unwrap();
        // Every fixed value, and a spread of the named ones.
        for (k, (item, count)) in items.iter().zip(&counts).enumerate() {
            if k >= 16 && k % 7 != 0 {
                continue;
            }
            let run = item.run_text(&names).unwrap();
            let ids = meguri_engine::browse::search::search_ids(&h.engine, &run, mode).unwrap();
            assert_eq!(*count, Some(ids.len() as u64), "{run:?} in {mode:?}");
        }
    }
}
