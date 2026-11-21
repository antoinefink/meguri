//! A search never writes (`browser/result-sets` H3, H9; `specs/conformance/
//! browser-ops.md` B1, B3): a session's sorted searches in both modes,
//! terms and sort keys that read the day, a resort, a reverse, row pages,
//! Question and Answer cells, a selection and the one-shot search leave
//! queues, stamps, `col` and every config row as they were, and commit no
//! op, although the day has moved past `lastUnburied` and a day query would
//! unbury the buried cards (`sched/day-clock` D7). The check at the end
//! shows it would.


// A selection is a list of index ranges; one range is the usual one.
#![allow(clippy::single_range_in_vec_init)]

mod common;

use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;

use meguri_engine::browse::search::{
    QaWhich, SearchEvent, SearchListener, SortDirection, SortSpec, search_ids,
};
use meguri_engine::sched;
use meguri_model::card::CardQueue;
use meguri_model::ids::CardId;
use meguri_search::SearchMode;
use meguri_storage::raw::RawRows;
use meguri_storage::{ReadRows, ReaderRole};

struct Events(Mutex<mpsc::Sender<SearchEvent>>);

impl SearchListener for Events {
    fn event(&self, event: SearchEvent) {
        let _ = self.0.lock().unwrap().send(event);
    }
}

const DAY_MS: i64 = 86_400_000;

fn dump(h: &common::Harness) -> Vec<RawRows> {
    let reader = h.collection.reader(ReaderRole::Query).unwrap();
    [
        "SELECT id, queue, type, due, mod, usn, data FROM cards ORDER BY id",
        "SELECT id, mod, usn, tags FROM notes ORDER BY id",
        "SELECT KEY, usn, mtime_secs, CAST(val AS TEXT) FROM config ORDER BY KEY",
        "SELECT mod, scm, usn, ls FROM col",
        "SELECT count(*) FROM revlog",
    ]
    .into_iter()
    .map(|sql| reader.execute_raw(sql, &[]).unwrap())
    .collect()
}

#[test]
fn a_session_and_the_one_shot_search_write_nothing_even_past_last_unburied() {
    let h = common::fresh();
    let kind = common::notes::kind(&h, false, &["Front", "Back"], &["{{Front}}", "{{Back}}"]);
    let mut cards = Vec::new();
    let mut notes = Vec::new();
    for index in 0..4 {
        let added = common::notes::add(&h, common::notes::input(kind, &[&format!("n{index}"), "b"]));
        notes.push(added.id.0);
        cards.extend(added.cards);
    }
    let buried: Vec<CardId> = cards[..3].to_vec();
    sched::ops::run(
        &h.collection,
        sched::ops::Request::Bury(sched::ops::Targets::Cards(buried.clone())),
        &Default::default(),
    )
    .unwrap();
    // Three days on, with no day check meanwhile.
    h.clock.set_now_ms(common::NOW_MS + 3 * DAY_MS);
    let before = dump(&h);
    let seq_before = h.collection.last_seq();

    let (sender, events) = mpsc::channel();
    let session = h.engine.open_search_session(Arc::new(Events(Mutex::new(sender))));
    let mut buried_ids: Vec<i64> = buried.iter().map(|card| card.0).collect();
    buried_ids.sort_unstable();
    let wait = |seq| loop {
        let event = events.recv_timeout(Duration::from_secs(30)).unwrap();
        if event.seq() == seq && event.is_terminal() {
            match event {
                SearchEvent::Complete { handle, count, revision, .. } => break (handle, count, revision),
                other => panic!("{other:?}"),
            }
        }
    };
    let searches = [
        ("is:buried", SearchMode::Cards, "cardDue", SortDirection::Ascending),
        ("is:buried", SearchMode::Cards, "cardDue", SortDirection::Descending),
        ("is:buried -prop:due>100000", SearchMode::Cards, "cardMod", SortDirection::Ascending),
        ("is:buried", SearchMode::Cards, "retrievability", SortDirection::Ascending),
        ("is:buried -is:due", SearchMode::Notes, "cardDue", SortDirection::Ascending),
    ];
    for (text, mode, key, direction) in searches {
        let seq = session.start_search(text, mode, SortSpec::new(key, direction), None);
        let (handle, count, revision) = wait(seq);
        let mut ids = session.resolve_selection(handle, revision, &[0..count], mode).unwrap();
        ids.sort_unstable();
        let expected = match mode {
            SearchMode::Cards => buried_ids.clone(),
            // Cards 0 to 2 are the two cards of note 0 and the first of note 1.
            SearchMode::Notes => notes[..2].to_vec(),
        };
        assert_eq!(ids, expected, "{text} by {key}");
        if mode == SearchMode::Cards {
            let seq = session
                .resort(handle, SortSpec::new("cardDue", SortDirection::Descending), None)
                .unwrap();
            let (sorted_again, count, _) = wait(seq);
            assert_eq!(count, 3);
            session.reverse(sorted_again, 0).unwrap();
            let rows = session
                .rows(sorted_again, 1, 0..count, &["cardDue", "retrievability", "question", "deck"])
                .unwrap();
            assert!(rows.iter().all(Result::is_ok));
            session.qa_cells(sorted_again, 1, 0..count, QaWhich::Both, None).unwrap();
        }
    }
    let mut one_shot = search_ids(&h.engine, "is:buried -is:due", SearchMode::Cards).unwrap();
    one_shot.sort_unstable();
    assert_eq!(one_shot, buried_ids);
    drop(session);

    assert_eq!(h.collection.last_seq(), seq_before, "no op committed");
    assert_eq!(dump(&h), before);

    // A day query now would have unburied them (D7).
    h.collection.check_rollover().unwrap();
    let reader = h.collection.reader(ReaderRole::Query).unwrap();
    let card = reader.card(buried[0]).unwrap().unwrap();
    assert_ne!(card.queue, CardQueue::BuriedByUser);
}
