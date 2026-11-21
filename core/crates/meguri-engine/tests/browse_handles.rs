//! `browser/result-sets` H1 to H16 through the engine's search sessions:
//! what no oracle sees (H16's last paragraph). Sessions, request seqs and
//! the order of their events, superseding and cancel tokens, handle
//! lifetime, revisions, page limits and deferred Question and Answer
//! cells, reverse and resort (no predicate run, rows that stopped matching
//! stay, set-aside cards), H10's index arithmetic and the removals recorded
//! while a request runs, selection resolution, the one-shot search and the
//! release of handles at a generation change.


// A selection is a list of index ranges; one range is the usual one.
#![allow(clippy::single_range_in_vec_init)]

mod common;

use std::fs;
use std::ops::Range;
use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;

use common::Harness;
use meguri_engine::browse::search::{
    BrowseError, HandleId, MapFrom, Mapped, QaWhich, Refetch, Selection, RequestSeq, SearchEvent, SearchListener, SearchSession, SortDirection,
    SortSpec, search_ids,
};
use meguri_engine::browse::{Answer, Cell, Question};
use meguri_engine::{CancelToken, EngineError, LongOp, OpResult, UndoScope, decks, notes, sched};
use meguri_model::changes::{ChangeSet, OpSource};
use meguri_model::ids::{CardId, DeckId, NoteId, NotetypeId};
use meguri_search::SearchMode;
use meguri_storage::{ReadRows, ReaderRole};

const WAIT: Duration = Duration::from_secs(30);

struct Events(Mutex<mpsc::Sender<SearchEvent>>);

impl SearchListener for Events {
    fn event(&self, event: SearchEvent) {
        let _ = self.0.lock().unwrap().send(event);
    }
}

/// A bootstrapped collection with `count` notes of a two-template note
/// type: note `i` has Front `n<i>` (zero-padded, so sort-field order is
/// note order) and two cards.
struct Fx {
    h: Harness,
    kind: NotetypeId,
    notes: Vec<NoteId>,
    cards: Vec<CardId>,
    session: SearchSession,
    events: mpsc::Receiver<SearchEvent>,
}

fn fixture(count: usize) -> Fx {
    let h = common::fresh();
    let kind = common::notes::kind(&h, false, &["Front", "Back"], &["{{Front}}", "{{Back}}"]);
    let mut notes = Vec::new();
    let mut cards = Vec::new();
    for index in 0..count {
        let added = common::notes::add(&h, common::notes::input(kind, &[&format!("n{index:04}"), "back"]));
        notes.push(added.id);
        cards.extend(added.cards);
    }
    let (session, events) = session(&h);
    Fx {
        h,
        kind,
        notes,
        cards,
        session,
        events,
    }
}

fn session(h: &Harness) -> (SearchSession, mpsc::Receiver<SearchEvent>) {
    let (sender, events) = mpsc::channel();
    (h.engine.open_search_session(Arc::new(Events(Mutex::new(sender)))), events)
}

fn asc(key: &str) -> SortSpec {
    SortSpec::new(key, SortDirection::Ascending)
}

fn desc(key: &str) -> SortSpec {
    SortSpec::new(key, SortDirection::Descending)
}

impl Fx {
    /// Every event up to and including `seq`'s terminal one.
    fn until(&self, seq: RequestSeq) -> Vec<SearchEvent> {
        let mut out = Vec::new();
        loop {
            let event = self.events.recv_timeout(WAIT).expect("an event");
            let done = event.seq() == seq && event.is_terminal();
            out.push(event);
            if done {
                return out;
            }
        }
    }

    /// Runs a search and returns its complete handle with its count.
    fn search(&self, text: &str, mode: SearchMode, sort: SortSpec) -> (HandleId, u64) {
        let seq = self.session.start_search(text, mode, sort, None);
        complete(&self.until(seq))
    }

    /// Every id of a complete handle, in order.
    fn ids(&self, handle: HandleId) -> Vec<i64> {
        let state = self.session.handle_state(handle).unwrap();
        self.session
            .resolve_selection(handle, state.revision, &[0..state.count], state.mode)
            .unwrap()
    }
}

fn complete(events: &[SearchEvent]) -> (HandleId, u64) {
    match events.last() {
        Some(SearchEvent::Complete { handle, count, .. }) => (*handle, *count),
        other => panic!("not complete: {other:?}"),
    }
}

fn card_ids(cards: &[CardId]) -> Vec<i64> {
    cards.iter().map(|card| card.0).collect()
}

#[test]
fn a_search_gives_a_first_page_then_complete_with_increasing_seqs_h3_h4() {
    let fx = fixture(150);
    let first = fx.session.start_search("", SearchMode::Cards, asc("noteFld"), None);
    assert_eq!(first, RequestSeq(1));
    let events = fx.until(first);
    let [
        SearchEvent::FirstPage { handle, ids, .. },
        SearchEvent::Complete {
            handle: done,
            count,
            revision,
            ..
        },
    ] = events.as_slice()
    else {
        panic!("{events:?}")
    };
    assert_eq!(handle, done);
    assert_eq!((*count, *revision), (300, 0));
    // Sort field, then the card's ordinal (S4): note order, two cards each.
    assert_eq!(ids.len(), 200);
    assert_eq!(fx.ids(*handle), card_ids(&fx.cards));
    assert_eq!(&fx.ids(*handle)[..200], ids.as_slice());
    let second = fx.session.start_search("", SearchMode::Notes, asc("noteFld"), None);
    assert_eq!(second, RequestSeq(2));
    let (notes, count) = complete(&fx.until(second));
    assert_eq!(count, 150);
    assert_eq!(fx.ids(notes), fx.notes.iter().map(|n| n.0).collect::<Vec<_>>());
}

#[test]
fn an_empty_result_is_an_empty_first_page_then_count_zero_h4() {
    let fx = fixture(3);
    let seq = fx.session.start_search("nid:1", SearchMode::Cards, asc("cardDue"), None);
    let events = fx.until(seq);
    assert!(
        matches!(events.as_slice(), [SearchEvent::FirstPage { ids, .. }, SearchEvent::Complete { count: 0, .. }] if ids.is_empty()),
        "{events:?}"
    );
}

#[test]
fn errors_come_as_failed_alone_in_b1_order_h3_h14() {
    let fx = fixture(3);
    let failed = |text: &str, sort: SortSpec| {
        let seq = fx.session.start_search(text, SearchMode::Cards, sort, None);
        match fx.until(seq).as_slice() {
            [SearchEvent::Failed { error, .. }] => error.clone(),
            other => panic!("{other:?}"),
        }
    };
    // A parse error first, then the column, then the run errors.
    assert!(matches!(failed("(", asc("question")), BrowseError::Search(e) if e.code() == "unclosed_group"));
    assert_eq!(failed("\"re:(\"", asc("question")), BrowseError::UnsortableColumn);
    assert_eq!(failed("", asc("nope")), BrowseError::UnsortableColumn);
    assert!(matches!(failed("\"re:(\"", asc("noteFld")), BrowseError::Search(e) if e.code() == "invalid_regex"));
    // The field column's sort key is U261's.
    assert!(matches!(
        failed("", asc("fld:Front")),
        BrowseError::Engine(EngineError::Unsupported { .. })
    ));
}

/// Holds every search reader, so a request waits for one.
fn hold_search_readers(h: &Harness) -> Vec<meguri_engine::ReaderLease<'_>> {
    (0..meguri_engine::readers::SEARCH_READERS)
        .map(|index| h.collection.search_reader(index).unwrap())
        .collect()
}

#[test]
fn a_new_request_supersedes_the_running_one_whose_events_all_come_first_h3_h4() {
    let fx = fixture(5);
    let held = hold_search_readers(&fx.h);
    let first = fx.session.start_search("", SearchMode::Cards, asc("cardDue"), None);
    std::thread::sleep(Duration::from_millis(50));
    let second = fx.session.start_search("", SearchMode::Notes, asc("noteFld"), None);
    drop(held);
    let events = fx.until(second);
    let seqs: Vec<(RequestSeq, &str)> = events
        .iter()
        .map(|event| {
            (
                event.seq(),
                match event {
                    SearchEvent::FirstPage { .. } => "first",
                    SearchEvent::Progress { .. } => "progress",
                    SearchEvent::Complete { .. } => "complete",
                    SearchEvent::Failed { .. } => "failed",
                    SearchEvent::Cancelled { .. } => "cancelled",
                },
            )
        })
        .collect();
    assert_eq!(
        seqs,
        vec![(first, "cancelled"), (second, "first"), (second, "complete")]
    );
    assert_eq!(fx.session.predicate_runs_for_testing(), 1);
}

#[test]
fn a_caller_token_cancels_its_request_j4() {
    let fx = fixture(5);
    let held = hold_search_readers(&fx.h);
    let token = CancelToken::new();
    let seq = fx.session.start_search("", SearchMode::Cards, asc("cardDue"), Some(&token));
    token.cancel();
    drop(held);
    assert!(matches!(fx.until(seq).as_slice(), [SearchEvent::Cancelled { .. }]));
    // An already cancelled token cancels before anything runs.
    let seq = fx.session.start_search("", SearchMode::Cards, asc("cardDue"), Some(&token));
    assert!(matches!(fx.until(seq).as_slice(), [SearchEvent::Cancelled { .. }]));
    assert_eq!(fx.session.predicate_runs_for_testing(), 0);
}

#[test]
fn superseding_after_first_page_releases_the_running_handle_h6_h12() {
    let fx = fixture(150);
    let (go, wait) = mpsc::channel::<()>();
    fx.session.after_first_page_for_testing(move || {
        let _ = wait.recv_timeout(WAIT);
    });
    let first = fx.session.start_search("", SearchMode::Cards, asc("noteFld"), None);
    let handle = match fx.events.recv_timeout(WAIT).unwrap() {
        SearchEvent::FirstPage { handle, seq, .. } if seq == first => handle,
        other => panic!("{other:?}"),
    };
    let second = fx.session.start_search("", SearchMode::Cards, desc("noteFld"), None);
    assert_eq!(fx.session.handle_state(handle), Err(BrowseError::UnknownHandle));
    go.send(()).unwrap();
    let events = fx.until(second);
    assert!(matches!(events[0], SearchEvent::Cancelled { seq } if seq == first), "{events:?}");
    assert!(matches!(events.last(), Some(SearchEvent::Complete { count: 300, .. })));
}

#[test]
fn a_session_keeps_the_two_newest_complete_handles_h6() {
    let fx = fixture(3);
    let (a, _) = fx.search("", SearchMode::Cards, asc("cardDue"));
    let (b, _) = fx.search("", SearchMode::Notes, asc("noteFld"));
    assert!(fx.session.handle_state(a).is_ok());
    let (c, _) = fx.search("", SearchMode::Cards, desc("cardDue"));
    assert_eq!(fx.session.handle_state(a), Err(BrowseError::UnknownHandle));
    assert!(fx.session.handle_state(b).is_ok());
    assert!(fx.session.handle_state(c).is_ok());
    // A handle of another session is unknown here, also when that session
    // holds handles of its own (H15).
    let (other, events) = session(&fx.h);
    let seq = other.start_search("", SearchMode::Notes, asc("noteFld"), None);
    let mut seen = Vec::new();
    loop {
        let event = events.recv_timeout(WAIT).unwrap();
        let done = event.seq() == seq && event.is_terminal();
        seen.push(event);
        if done {
            break;
        }
    }
    let (own, _) = complete(&seen);
    assert!(other.handle_state(own).is_ok());
    for handle in [a, b, c] {
        assert_eq!(other.handle_state(handle), Err(BrowseError::UnknownHandle));
    }
    assert_eq!(fx.session.handle_state(own), Err(BrowseError::UnknownHandle));
}

#[test]
fn a_handle_in_progress_pages_its_prefix_and_keeps_out_rows_deleted_beyond_it_h6_h8_h10() {
    let fx = fixture(150);
    let (go, wait) = mpsc::channel::<()>();
    let (paused, reached) = mpsc::channel::<()>();
    fx.session.after_first_page_for_testing(move || {
        paused.send(()).unwrap();
        let _ = wait.recv_timeout(WAIT);
    });
    let seq = fx.session.start_search("", SearchMode::Cards, asc("noteFld"), None);
    let handle = match fx.events.recv_timeout(WAIT).unwrap() {
        SearchEvent::FirstPage { handle, ids, .. } => {
            assert_eq!(ids.len(), 200);
            handle
        }
        other => panic!("{other:?}"),
    };
    reached.recv_timeout(WAIT).unwrap();
    let state = fx.session.handle_state(handle).unwrap();
    assert_eq!((state.complete, state.count, state.revision), (false, 200, 0));
    assert_eq!(fx.session.reverse(handle, 0), Err(BrowseError::NotComplete));
    assert_eq!(
        fx.session.resort(handle, asc("cardDue"), None),
        Err(BrowseError::NotComplete)
    );
    assert_eq!(
        fx.session.rows(handle, 0, 199..201, &["noteFld"]).err(),
        Some(BrowseError::OutOfRange)
    );
    assert_eq!(fx.session.rows(handle, 0, 0..10, &["noteFld"]).unwrap().len(), 10);
    // Delete the note of cards 0 and 1 (in the prefix) and of the last two
    // cards (beyond it).
    let deleted = notes::delete(
        &fx.h.collection,
        vec![fx.notes[0], fx.notes[149]],
        OpSource::UserUndoable,
    )
    .unwrap()
    .unwrap();
    let invalidation = fx
        .session
        .invalidated_rows(handle, 0, &[(deleted.seq, &deleted.changes)])
        .unwrap();
    assert_eq!(invalidation.removed, vec![0, 1]);
    assert_eq!((invalidation.revision, invalidation.complete, invalidation.count), (1, false, 198));
    go.send(()).unwrap();
    match fx.until(seq).last() {
        Some(SearchEvent::Complete { count, revision, .. }) => assert_eq!((*count, *revision), (296, 1)),
        other => panic!("{other:?}"),
    }
    let ids = fx.ids(handle);
    assert_eq!(ids, card_ids(&fx.cards[2..298]));
}

#[test]
fn reverse_flips_the_list_and_equals_the_opposite_direction_h8() {
    let fx = fixture(20);
    let (up, count) = fx.search("", SearchMode::Cards, asc("cardMod"));
    let (down, _) = fx.search("", SearchMode::Cards, desc("cardMod"));
    let mut expected = fx.ids(down);
    assert_eq!(fx.session.reverse(up, 0), Ok(1));
    assert_eq!(fx.ids(up), expected);
    let state = fx.session.handle_state(up).unwrap();
    assert_eq!((state.direction, state.count), (SortDirection::Descending, count));
    assert_eq!(fx.session.reverse(up, 0), Err(BrowseError::StaleRevision { current: 1 }));
    assert_eq!(fx.session.reverse(up, 1), Ok(2));
    expected.reverse();
    assert_eq!(fx.ids(up), expected);
}

#[test]
fn pages_check_revisions_limits_and_columns_and_defer_question_and_answer_h7() {
    let fx = fixture(600);
    let (handle, count) = fx.search("", SearchMode::Cards, asc("noteFld"));
    assert_eq!(count, 1200);
    assert_eq!(
        fx.session.rows(handle, 3, 0..1, &["noteFld"]).err(),
        Some(BrowseError::StaleRevision { current: 0 })
    );
    assert!(matches!(
        fx.session.rows(handle, 0, 0..1001, &["noteFld"]).err(),
        Some(BrowseError::InvalidArgument { .. })
    ));
    assert_eq!(fx.session.rows(handle, 0, 100..1100, &["noteFld"]).unwrap().len(), 1000);
    assert!(matches!(
        fx.session.rows(handle, 0, 0..1, &["nope"]).err(),
        Some(BrowseError::InvalidArgument { .. })
    ));
    assert!(matches!(
        fx.session.rows(handle, 0, 0..1, &["fld:Front"]).err(),
        Some(BrowseError::Engine(EngineError::Unsupported { .. }))
    ));
    assert_eq!(
        fx.session.rows(handle, 0, 1199..1201, &["noteFld"]).err(),
        Some(BrowseError::OutOfRange)
    );
    let rows = fx
        .session
        .rows(handle, 0, 2..4, &["question", "noteFld", "answer", "template"])
        .unwrap();
    let row = rows[0].as_ref().unwrap();
    assert_eq!(row.id, fx.cards[2].0);
    assert_eq!(
        row.cells,
        vec![
            Cell::Deferred,
            Cell::Text("n0001".into()),
            Cell::Deferred,
            Cell::Text("View 0".into())
        ]
    );
    let cells = fx.session.qa_cells(handle, 0, 2..4, QaWhich::Both, None).unwrap();
    let first = cells[0].as_ref().unwrap();
    assert_eq!(first.question, Some(Question::Text("n0001".into())));
    assert_eq!(first.answer, Some(Answer::Text(String::new())));
    let second = cells[1].as_ref().unwrap();
    assert_eq!(second.question, Some(Question::Text("back".into())));
    let only = fx.session.qa_cells(handle, 0, 2..3, QaWhich::Answer, None).unwrap();
    assert_eq!(only[0].as_ref().unwrap().question, None);
    let token = CancelToken::new();
    token.cancel();
    assert_eq!(
        fx.session.qa_cells(handle, 0, 0..2, QaWhich::Question, Some(&token)).err(),
        Some(BrowseError::Engine(EngineError::Cancelled))
    );
}

#[test]
fn rows_are_read_at_the_call_and_deleted_rows_are_row_errors_until_pruned_h7() {
    let fx = fixture(3);
    let (handle, _) = fx.search("", SearchMode::Cards, asc("noteFld"));
    common::set_first_field(&fx.h.collection, OpSource::UserUndoable, fx.notes[0], "edited").unwrap();
    notes::delete(&fx.h.collection, vec![fx.notes[2]], OpSource::UserUndoable).unwrap();
    let rows = fx.session.rows(handle, 0, 0..6, &["noteFld"]).unwrap();
    assert_eq!(rows[0].as_ref().unwrap().cells, vec![Cell::Text("edited".into())]);
    assert_eq!(rows[4], Err(meguri_engine::browse::RowFailure::Deleted));
}

/// Every `Changes` event of `subscription` so far, as H10 takes them.
fn changes(subscription: &meguri_engine::Subscription) -> Vec<(meguri_engine::Seq, Arc<ChangeSet>)> {
    common::drain(subscription, Duration::from_millis(100))
        .into_iter()
        .filter_map(|event| match &event.kind {
            meguri_engine::EventKind::Changes(changes) => Some((event.seq, Arc::clone(changes))),
            _ => None,
        })
        .collect()
}

fn borrowed(sets: &[(meguri_engine::Seq, Arc<ChangeSet>)]) -> Vec<(meguri_engine::Seq, &ChangeSet)> {
    sets.iter().map(|(seq, changes)| (*seq, &**changes)).collect()
}

#[test]
fn change_sets_remove_and_refetch_rows_by_index_and_apply_once_h10() {
    let fx = fixture(6);
    let stream = fx.h.engine.stream().subscribe();
    let (cards, _) = fx.search("", SearchMode::Cards, asc("noteFld"));
    let (notes_handle, _) = fx.search("", SearchMode::Notes, asc("noteFld"));
    // An edit of note 1 refetches its two cards, in the new revision.
    common::set_first_field(&fx.h.collection, OpSource::UserUndoable, fx.notes[1], "n0001x").unwrap();
    let edit = changes(&stream);
    let result = fx.session.invalidated_rows(cards, 0, &borrowed(&edit)).unwrap();
    assert_eq!((result.removed.clone(), result.refetch.clone()), (vec![], Refetch::Rows(vec![2, 3])));
    // Applied once: the same change set again changes nothing.
    let again = fx.session.invalidated_rows(cards, 0, &borrowed(&edit)).unwrap();
    assert_eq!(again.refetch, Refetch::Rows(vec![]));
    // A deletion removes rows and moves the revision; the refetch indexes
    // are of the new revision.
    let deleted = notes::delete(&fx.h.collection, vec![fx.notes[0]], OpSource::UserUndoable).unwrap();
    assert!(deleted.is_some());
    common::set_first_field(&fx.h.collection, OpSource::UserUndoable, fx.notes[2], "n0002x").unwrap();
    let later = changes(&stream);
    let result = fx.session.invalidated_rows(cards, 0, &borrowed(&later)).unwrap();
    assert_eq!(result.removed, vec![0, 1]);
    assert_eq!(result.refetch, Refetch::Rows(vec![2, 3]));
    assert_eq!((result.revision, result.count, result.complete), (1, 10, true));
    // Notes mode: the edited notes refetch, the deleted note goes.
    let mut all = edit.clone();
    all.extend(later.iter().cloned());
    let result = fx.session.invalidated_rows(notes_handle, 0, &borrowed(&all)).unwrap();
    assert_eq!(result.removed, vec![0]);
    assert_eq!(result.refetch, Refetch::Rows(vec![0, 1]));
    // Undo is an ordinary change set: the restored note does not come back.
    fx.h.collection.undo(UndoScope::Global).unwrap();
    fx.h.collection.undo(UndoScope::Global).unwrap();
    let undone = changes(&stream);
    let result = fx.session.invalidated_rows(cards, 1, &borrowed(&undone)).unwrap();
    assert_eq!((result.revision, result.count), (1, 10));
    // A deck rename reaches every row.
    let renamed: OpResult<_> = decks::rename(&fx.h.collection, DeckId(1), "Renamed".into()).unwrap();
    let result = fx
        .session
        .invalidated_rows(cards, 1, &[(renamed.seq, &renamed.changes)])
        .unwrap();
    assert_eq!(result.refetch, Refetch::All);
    assert_eq!(
        fx.session.invalidated_rows(cards, 0, &[]).err(),
        Some(BrowseError::StaleRevision { current: 1 })
    );
}

#[test]
fn a_resort_sorts_the_ids_without_the_predicate_and_keeps_rows_that_stopped_matching_h8() {
    let fx = fixture(10);
    let (handle, count) = fx.search("-is:suspended", SearchMode::Cards, asc("noteFld"));
    assert_eq!(count, 20);
    let runs = fx.session.predicate_runs_for_testing();
    sched::ops::run(
        &fx.h.collection,
        sched::ops::Request::Suspend(sched::ops::Targets::Cards(vec![fx.cards[4]])),
        &Default::default(),
    )
    .unwrap();
    let seq = fx.session.resort(handle, desc("noteFld"), None).unwrap();
    let (sorted_again, count) = complete(&fx.until(seq));
    assert_eq!(count, 20, "the suspended card stays");
    assert_eq!(fx.session.predicate_runs_for_testing(), runs, "no predicate run");
    let mut expected = card_ids(&fx.cards);
    // noteFld descending, then ord descending (S8's final key in the
    // sort's direction).
    expected.reverse();
    assert_eq!(fx.ids(sorted_again), expected);
    // The source handle stays usable: it is one of the two newest.
    assert!(fx.session.handle_state(handle).is_ok());
    let seq = fx.session.resort(sorted_again, asc("nope"), None).unwrap();
    assert!(matches!(
        fx.until(seq).as_slice(),
        [SearchEvent::Failed {
            error: BrowseError::UnsortableColumn,
            ..
        }]
    ));
}

#[test]
fn resolve_selection_gives_the_ids_an_action_works_on_h12() {
    let fx = fixture(4);
    let (cards, _) = fx.search("", SearchMode::Cards, asc("noteFld"));
    let (notes_handle, _) = fx.search("", SearchMode::Notes, desc("noteFld"));
    let ranges: [Range<u64>; 2] = [5..7, 1..2];
    assert_eq!(
        fx.session.resolve_selection(cards, 0, &ranges, SearchMode::Cards).unwrap(),
        vec![fx.cards[1].0, fx.cards[5].0, fx.cards[6].0]
    );
    assert_eq!(
        fx.session.resolve_selection(cards, 0, &ranges, SearchMode::Notes).unwrap(),
        vec![fx.notes[0].0, fx.notes[2].0, fx.notes[3].0]
    );
    // Notes descending: note 3, 2, 1, 0; each note's cards ascending.
    assert_eq!(
        fx.session.resolve_selection(notes_handle, 0, &[0..2], SearchMode::Cards).unwrap(),
        card_ids(&[fx.cards[6], fx.cards[7], fx.cards[4], fx.cards[5]])
    );
    assert_eq!(
        fx.session.resolve_selection(notes_handle, 0, &[3..5], SearchMode::Notes).err(),
        Some(BrowseError::OutOfRange)
    );
}

#[test]
fn the_one_shot_search_lists_ids_ascending_with_failed_s_errors_h13() {
    let fx = fixture(5);
    let mut cards = card_ids(&fx.cards);
    cards.sort_unstable();
    assert_eq!(search_ids(&fx.h.engine, "", SearchMode::Cards).unwrap(), cards);
    assert_eq!(
        search_ids(&fx.h.engine, "n0003", SearchMode::Notes).unwrap(),
        vec![fx.notes[3].0]
    );
    let one_shot = search_ids(&fx.h.engine, "deck:x (", SearchMode::Cards).unwrap_err();
    let seq = fx.session.start_search("deck:x (", SearchMode::Cards, asc("cardDue"), None);
    match fx.until(seq).as_slice() {
        [SearchEvent::Failed {
            error: BrowseError::Search(failed),
            ..
        }] => {
            let BrowseError::Search(one_shot) = one_shot else {
                panic!("{one_shot:?}")
            };
            assert_eq!(one_shot.code(), failed.code());
            assert_eq!(one_shot.message(), failed.message());
            assert_eq!(one_shot.range(), failed.range());
        }
        other => panic!("{other:?}"),
    }
    let _ = fx.kind;
}

#[test]
fn closing_a_session_cancels_its_request_and_releases_its_handles_h2() {
    let fx = fixture(3);
    let (handle, _) = fx.search("", SearchMode::Cards, asc("cardDue"));
    let held = hold_search_readers(&fx.h);
    let seq = fx.session.start_search("", SearchMode::Cards, asc("cardDue"), None);
    let events = fx.events;
    let probe = std::thread::spawn(move || {
        let mut out = Vec::new();
        while let Ok(event) = events.recv_timeout(WAIT) {
            let done = event.seq() == seq && event.is_terminal();
            out.push(event);
            if done {
                break;
            }
        }
        out
    });
    fx.session.close();
    drop(held);
    assert!(matches!(probe.join().unwrap().as_slice(), [SearchEvent::Cancelled { .. }]));
    let (other, _events) = session(&fx.h);
    assert_eq!(other.handle_state(handle), Err(BrowseError::UnknownHandle));
}

#[test]
fn a_collection_replacement_releases_every_handle_of_the_old_generation_h6() {
    let fx = fixture(3);
    let (handle, _) = fx.search("", SearchMode::Cards, asc("cardDue"));
    let source = fx.h.dir.path().join("replacement.anki2");
    fs::copy(&fx.h.path, &source).unwrap();
    let slot = fx.h.engine.acquire_slot(LongOp::Restore).unwrap();
    let mut exclusive = slot.enter_exclusive(true).unwrap();
    exclusive
        .replace(move |path| fs::rename(source, path).map_err(|e| EngineError::Storage(e.into())))
        .unwrap();
    drop(exclusive);
    assert_eq!(fx.session.handle_state(handle), Err(BrowseError::UnknownHandle));
    assert_eq!(
        fx.session.rows(handle, 0, 0..1, &["noteFld"]).err(),
        Some(BrowseError::UnknownHandle)
    );
    // The session goes on in the new generation.
    let (fresh, _) = fx.search("", SearchMode::Cards, asc("cardDue"));
    assert!(fx.session.handle_state(fresh).is_ok());
}

/// A card whose note row is missing (Q6), added raw before the engine opens.
fn damaged() -> Fx {
    let fx = fixture(3);
    let Fx { h, kind, notes, cards, .. } = fx;
    let Harness { dir, path, engine, .. } = h;
    engine.close().unwrap();
    let clock = meguri_model::clock::StepClock::new(common::NOW_MS, meguri_model::clock::UtcOffset::UTC, "UTC");
    common::day::raw(
        &path,
        &clock,
        &[(
            "INSERT INTO cards SELECT id + 1000, 4242, did, ord, mod, usn, type, queue, due + 50, ivl, factor, reps, \
             lapses, left, odue, odid, flags, data FROM cards WHERE id = (SELECT min(id) FROM cards)",
            Vec::new(),
        ), (
            // A note without cards, `n0000` as the first note.
            "INSERT INTO notes SELECT id + 5000, guid || 'x', mid, mod, usn, tags, flds, sfld, csum, flags, data \
             FROM notes WHERE id = (SELECT min(id) FROM notes)",
            Vec::new(),
        )],
    );
    let h = Harness::open_path(dir, path);
    let (session, events) = session(&h);
    Fx {
        h,
        kind,
        notes,
        cards,
        session,
        events,
    }
}

#[test]
fn a_note_column_sort_sets_lone_cards_aside_and_a_resort_brings_them_back_h5_h8() {
    let fx = damaged();
    let lone = fx.cards[0].0 + 1000;
    let (by_field, count) = fx.search("is:new", SearchMode::Cards, asc("noteFld"));
    assert_eq!(count, 6);
    assert!(!fx.ids(by_field).contains(&lone));
    let seq = fx.session.resort(by_field, asc("cardDue"), None).unwrap();
    let (by_due, count) = complete(&fx.until(seq));
    assert_eq!(count, 7);
    let direct = fx.search("is:new", SearchMode::Cards, asc("cardDue")).0;
    assert_eq!(fx.ids(by_due), fx.ids(direct));
    assert!(fx.ids(by_due).contains(&lone));
    let seq = fx.session.resort(by_due, asc("noteFld"), None).unwrap();
    let (back, count) = complete(&fx.until(seq));
    assert_eq!(count, 6);
    let seq = fx.session.resort(back, asc("cardDue"), None).unwrap();
    let (again, _) = complete(&fx.until(seq));
    assert!(fx.ids(again).contains(&lone), "kept aside, not forgotten");
    // A query with note scope matches no card without a note.
    let (scoped, count) = fx.search("Front:*", SearchMode::Cards, asc("cardDue"));
    assert_eq!(count, 6);
    assert!(!fx.ids(scoped).contains(&lone));
    let reader = fx.h.collection.reader(ReaderRole::Query).unwrap();
    assert!(reader.card(CardId(lone)).unwrap().is_some());
}

impl Fx {
    /// Toggles a complete handle and returns the new complete handle.
    fn toggle(&self, handle: HandleId, sort: SortSpec) -> (HandleId, u64) {
        let seq = self.session.toggle(handle, sort, None).unwrap();
        complete(&self.until(seq))
    }
}

/// H11's table on a sound collection: every scope but "either" derives the
/// other mode's set with no predicate run and no extra test, and the set
/// equals the search run in that mode; "either" runs the query.
#[test]
fn a_toggle_derives_the_other_mode_without_a_run_on_a_sound_collection_h11() {
    let fx = fixture(12);
    sched::ops::run(
        &fx.h.collection,
        sched::ops::Request::Suspend(sched::ops::Targets::Cards(vec![fx.cards[3], fx.cards[8]])),
        &Default::default(),
    )
    .unwrap();
    // Card scope, note scope, card and note, either.
    let cases = [("is:suspended", true), ("n0001 or n0004", true), ("is:suspended n0004", true), ("", false)];
    for (text, derived) in cases {
        for (from, to, from_key, to_key) in [
            (SearchMode::Cards, SearchMode::Notes, "cardDue", "noteFld"),
            (SearchMode::Notes, SearchMode::Cards, "noteFld", "cardDue"),
        ] {
            let (handle, _) = fx.search(text, from, asc(from_key));
            let runs = fx.session.predicate_runs_for_testing();
            let (toggled, _) = fx.toggle(handle, desc(to_key));
            let ran = fx.session.predicate_runs_for_testing() - runs;
            assert_eq!(ran, u64::from(!derived), "{text:?} {from:?}");
            let (direct, _) = fx.search(text, to, desc(to_key));
            assert_eq!(fx.ids(toggled), fx.ids(direct), "{text:?} {from:?}");
            assert_eq!(fx.session.handle_state(toggled).unwrap().mode, to);
        }
    }
    assert_eq!(fx.session.extra_tests_for_testing(), 0);
}

#[test]
fn a_toggle_runs_the_query_after_a_commit_or_a_changed_context_and_refuses_a_handle_in_progress_h11() {
    let fx = fixture(4);
    let (handle, _) = fx.search("n0001", SearchMode::Cards, asc("cardDue"));
    common::set_first_field(&fx.h.collection, OpSource::UserUndoable, fx.notes[2], "n0001 too").unwrap();
    let runs = fx.session.predicate_runs_for_testing();
    let (toggled, count) = fx.toggle(handle, asc("noteFld"));
    assert_eq!(fx.session.predicate_runs_for_testing(), runs + 1, "an op committed after the match seq");
    assert_eq!(count, 2);
    assert_eq!(fx.ids(toggled), vec![fx.notes[1].0, fx.notes[2].0]);
    // `is:due` reads now: a second later the context differs.
    let (due, _) = fx.search("is:due or n0003", SearchMode::Cards, asc("cardDue"));
    fx.h.clock.set_now_ms(common::NOW_MS + 2_000);
    let runs = fx.session.predicate_runs_for_testing();
    fx.toggle(due, asc("noteFld"));
    assert_eq!(fx.session.predicate_runs_for_testing(), runs + 1, "the context changed");
    // In progress: NotComplete, and the request goes on.
    let (go, wait) = mpsc::channel::<()>();
    fx.session.after_first_page_for_testing(move || {
        let _ = wait.recv_timeout(WAIT);
    });
    let seq = fx.session.start_search("", SearchMode::Cards, asc("cardDue"), None);
    let handle = match fx.events.recv_timeout(WAIT).unwrap() {
        SearchEvent::FirstPage { handle, .. } => handle,
        other => panic!("{other:?}"),
    };
    assert_eq!(fx.session.toggle(handle, asc("noteFld"), None), Err(BrowseError::NotComplete));
    go.send(()).unwrap();
    assert!(matches!(fx.until(seq).last(), Some(SearchEvent::Complete { count: 8, .. })));
    let seq = fx.session.toggle(handle, asc("nope"), None).unwrap();
    assert!(matches!(
        fx.until(seq).as_slice(),
        [SearchEvent::Failed {
            error: BrowseError::UnsortableColumn,
            ..
        }]
    ));
}

/// H11's extra rows on a collection with a card without a note and a note
/// without cards (`toggle-damaged`, `sorted-damaged`), and S9 in the derived
/// cards mode.
#[test]
fn a_toggle_on_a_damaged_collection_adds_exactly_the_lone_rows_a_run_gives_h5_h11() {
    let fx = damaged();
    let lone_card = fx.cards[0].0 + 1000;
    let lone_note = fx.notes[0].0 + 5000;
    let check = |text: &str, from: SearchMode, from_key: &str, to_key: &str| {
        let to = match from {
            SearchMode::Cards => SearchMode::Notes,
            SearchMode::Notes => SearchMode::Cards,
        };
        let (handle, _) = fx.search(text, from, asc(from_key));
        let runs = fx.session.predicate_runs_for_testing();
        let (toggled, _) = fx.toggle(handle, asc(to_key));
        assert_eq!(fx.session.predicate_runs_for_testing(), runs, "{text} {from:?}: derived");
        let (direct, _) = fx.search(text, to, asc(to_key));
        assert_eq!(fx.ids(toggled), fx.ids(direct), "{text} {from:?} to {to_key}");
        fx.ids(toggled)
    };
    // Cards to notes, card scope: no note for the lone card.
    let notes = check("is:new", SearchMode::Cards, "cardDue", "noteFld");
    assert!(!notes.contains(&4242));
    // Notes to cards, card scope: the lone card joins under cardDue and is
    // set aside under noteFld, as a run does.
    let extra = fx.session.extra_tests_for_testing();
    assert!(check("is:new", SearchMode::Notes, "noteFld", "cardDue").contains(&lone_card));
    assert!(!check("is:new", SearchMode::Notes, "noteFld", "noteFld").contains(&lone_card));
    assert_eq!(fx.session.extra_tests_for_testing(), extra + 2);
    // Cards to notes, note scope: the note without cards joins.
    assert!(check("n0000", SearchMode::Cards, "cardDue", "noteFld").contains(&lone_note));
    // A derived cards handle keeps the lone card aside and a resort brings
    // it back (H5, H8).
    let (notes_handle, _) = fx.search("is:new", SearchMode::Notes, asc("noteFld"));
    let (by_field, count) = fx.toggle(notes_handle, asc("noteFld"));
    assert_eq!(count, 6);
    let seq = fx.session.resort(by_field, asc("cardDue"), None).unwrap();
    let (by_due, count) = complete(&fx.until(seq));
    assert_eq!(count, 7);
    assert!(fx.ids(by_due).contains(&lone_card));
}

#[test]
fn index_of_finds_ids_of_either_kind_h12() {
    let fx = fixture(3);
    let (cards, _) = fx.search("", SearchMode::Cards, desc("noteFld"));
    let (notes_handle, _) = fx.search("", SearchMode::Notes, asc("noteFld"));
    // Cards descending: note 2's cards first.
    assert_eq!(fx.session.index_of(cards, 0, SearchMode::Cards, fx.cards[5].0), Ok(Some(0)));
    assert_eq!(fx.session.index_of(cards, 0, SearchMode::Notes, fx.notes[1].0), Ok(Some(2)));
    assert_eq!(fx.session.index_of(notes_handle, 0, SearchMode::Cards, fx.cards[5].0), Ok(Some(2)));
    assert_eq!(fx.session.index_of(notes_handle, 0, SearchMode::Notes, fx.notes[0].0), Ok(Some(0)));
    // Card ids and note ids are separate spaces.
    if !fx.notes.iter().any(|note| note.0 == fx.cards[0].0) {
        assert_eq!(fx.session.index_of(notes_handle, 0, SearchMode::Notes, fx.cards[0].0), Ok(None));
    }
    assert_eq!(fx.session.index_of(cards, 0, SearchMode::Cards, -1), Ok(None));
    assert_eq!(
        fx.session.index_of(cards, 1, SearchMode::Cards, -1),
        Err(BrowseError::StaleRevision { current: 0 })
    );
}

#[test]
fn map_selection_follows_rows_into_a_re_search_and_a_toggle_h12() {
    let fx = fixture(4);
    sched::ops::run(
        &fx.h.collection,
        sched::ops::Request::Suspend(sched::ops::Targets::Cards(vec![fx.cards[2]])),
        &Default::default(),
    )
    .unwrap();
    let (all, _) = fx.search("", SearchMode::Cards, asc("noteFld"));
    let (active, _) = fx.search("-is:suspended", SearchMode::Cards, asc("noteFld"));
    // A re-search: cards 1 to 3 selected, card 2 current; card 2 is gone.
    let mapped = fx
        .session
        .map_selection(
            MapFrom::Handle { handle: all, revision: 0 },
            Selection::Ranges(vec![1..4]),
            Some(fx.cards[2].0),
            active,
            0,
        )
        .unwrap();
    assert_eq!(mapped, Mapped { selected: vec![1..3], current: None });
    // Cards to notes: the notes of the selected cards.
    let (notes_handle, _) = fx.search("", SearchMode::Notes, asc("noteFld"));
    let mapped = fx
        .session
        .map_selection(
            MapFrom::Handle { handle: active, revision: 0 },
            Selection::Ranges(vec![1..3]),
            Some(fx.cards[3].0),
            notes_handle,
            0,
        )
        .unwrap();
    assert_eq!(mapped, Mapped { selected: vec![0..2], current: Some(1) });
    // Notes to cards: every card of the selected notes in `to`; the current
    // note's first card, or its lowest-id card in `to` when the first is
    // not there (card 2 is note 1's first card and is suspended).
    let mapped = fx
        .session
        .map_selection(
            MapFrom::Handle { handle: notes_handle, revision: 0 },
            Selection::Ranges(vec![1..2]),
            Some(fx.notes[1].0),
            active,
            0,
        )
        .unwrap();
    assert_eq!(mapped, Mapped { selected: vec![2..3], current: Some(2) });
    // From a released handle: ids the caller holds.
    let mapped = fx
        .session
        .map_selection(
            MapFrom::Mode(SearchMode::Cards),
            Selection::Ids(vec![fx.cards[6].0, fx.cards[0].0]),
            Some(fx.cards[6].0),
            notes_handle,
            0,
        )
        .unwrap();
    assert_eq!(mapped, Mapped { selected: vec![0..1, 3..4], current: Some(3) });
    assert!(matches!(
        fx.session.map_selection(MapFrom::Mode(SearchMode::Cards), Selection::Ranges(vec![0..1]), None, notes_handle, 0),
        Err(BrowseError::InvalidArgument { .. })
    ));
}

/// Q10 through a session, one candidate per chunk: the streamed list equals
/// the same rows sorted without the predicate, the first page goes out
/// while the run goes on, a note deleted beyond the prefix never joins it
/// (H10), and the one-shot search streams to the same set (H13).
#[test]
fn a_slow_search_streams_and_equals_the_whole_statement_q10_h3_h10_h13() {
    let fx = fixture(150);
    fx.h.engine.set_search_stream_chunk_for_testing(1);
    let (handle, count) = fx.search("re:n00[0-4]", SearchMode::Cards, desc("noteFld"));
    assert_eq!(count, 100);
    let seq = fx.session.resort(handle, desc("noteFld"), None).unwrap();
    let (sorted_again, _) = complete(&fx.until(seq));
    assert_eq!(fx.ids(handle), fx.ids(sorted_again));
    // Notes mode with card scope keeps notes through their pairs.
    let (notes_handle, count) = fx.search("re:n00[0-4] -is:new", SearchMode::Notes, asc("noteFld"));
    assert_eq!(count, 0);
    assert!(fx.session.handle_state(notes_handle).unwrap().complete);
    // The first page is final before the run ends.
    let (go, wait) = mpsc::channel::<()>();
    fx.session.after_first_page_for_testing(move || {
        let _ = wait.recv_timeout(WAIT);
    });
    let seq = fx.session.start_search("n0", SearchMode::Cards, asc("noteFld"), None);
    let (handle, first) = match fx.events.recv_timeout(WAIT).unwrap() {
        SearchEvent::FirstPage { handle, ids, .. } => (handle, ids),
        other => panic!("{other:?}"),
    };
    assert_eq!(first, card_ids(&fx.cards[..200]));
    assert!(!fx.session.handle_state(handle).unwrap().complete);
    let deleted = notes::delete(&fx.h.collection, vec![fx.notes[149]], OpSource::UserUndoable)
        .unwrap()
        .unwrap();
    fx.session
        .invalidated_rows(handle, 0, &[(deleted.seq, &deleted.changes)])
        .unwrap();
    go.send(()).unwrap();
    let events = fx.until(seq);
    assert!(matches!(events.last(), Some(SearchEvent::Complete { count: 298, .. })), "{events:?}");
    assert_eq!(fx.ids(handle), card_ids(&fx.cards[..298]));
    // The one-shot search streams to every matching id, ascending.
    let streamed = search_ids(&fx.h.engine, "re:n00[0-4]", SearchMode::Notes).unwrap();
    let mut expected: Vec<i64> = fx.notes[..50].iter().map(|note| note.0).collect();
    expected.sort_unstable();
    assert_eq!(streamed, expected);
    // In notes mode with card scope a note is kept through one of its
    // card-and-note pairs.
    let (pairs, count) = fx.search("re:n00[0-4] card:1", SearchMode::Notes, asc("noteFld"));
    assert_eq!(count, 50);
    assert_eq!(fx.ids(pairs), fx.notes[..50].iter().map(|note| note.0).collect::<Vec<_>>());
}

/// H15: handle ids are unique across engines too; each session refuses
/// the other's handle although both hold results.
#[test]
fn sessions_on_two_engines_refuse_each_others_handles_h15() {
    let first = fixture(2);
    let second = fixture(3);
    let (a, _) = first.search("", SearchMode::Cards, asc("cardDue"));
    let (b, _) = second.search("", SearchMode::Cards, asc("cardDue"));
    assert_ne!(a, b);
    assert_eq!(second.session.handle_state(a), Err(BrowseError::UnknownHandle));
    assert_eq!(first.session.handle_state(b), Err(BrowseError::UnknownHandle));
    assert_eq!(first.session.handle_state(a).unwrap().count, 4);
    assert_eq!(second.session.handle_state(b).unwrap().count, 6);
}

/// H3: a request made while searches are stopping (a replacement, a suspend
/// or a close in progress) fails with the engine's refusal, even when its
/// worker starts after the halt ends; the one-shot search is refused too.
#[test]
fn a_request_made_during_a_halt_fails_with_the_engines_refusal_h3() {
    let fx = fixture(2);
    let held = hold_search_readers(&fx.h);
    let seq = SearchSession::while_searches_stopped_for_testing(&fx.h.engine, || {
        assert!(matches!(
            search_ids(&fx.h.engine, "", SearchMode::Cards),
            Err(BrowseError::Engine(EngineError::CollectionUnavailable { .. }))
        ));
        fx.session.start_search("", SearchMode::Cards, asc("cardDue"), None)
    });
    drop(held);
    assert!(matches!(
        fx.until(seq).as_slice(),
        [SearchEvent::Failed {
            error: BrowseError::Engine(EngineError::CollectionUnavailable { .. }),
            ..
        }]
    ));
    // Afterwards searches run again.
    assert_eq!(fx.search("", SearchMode::Cards, asc("cardDue")).1, 4);
}
