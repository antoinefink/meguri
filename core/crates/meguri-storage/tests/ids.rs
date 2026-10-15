//! Ids of new rows (`storage/ids-usn-mtime` I2, I3; R772, R1469, R1470).
//! Each test replays a storage scenario's writes through write scopes and
//! compares the ids with the oracle's golden, with the deviations of
//! `conformance/deviations.yaml` applied where the one-day cap differs from
//! Anki.

mod common;

use common::{NOW_MS, card, deck, empty_collection, expected_steps, note, notetype, open, preset, revlog};
use meguri_model::clock::{StepClock, UtcOffset};
use meguri_model::ids::{CardId, NoteId};
use meguri_storage::{JournalOptions, OpenedCollection, StampMode};
use serde_json::{Map, Value, json};
use tempfile::TempDir;

const DAY_MS: i64 = 86_400_000;

/// A builder-made empty collection, its clock at the scenarios' start, and
/// the logical ids of the notes and cards added so far.
struct Run {
    collection: OpenedCollection,
    clock: StepClock,
    notes: Vec<NoteId>,
    cards: Vec<CardId>,
    revlog: Vec<(usize, i64)>,
    _dir: TempDir,
}

impl Run {
    fn new() -> Run {
        let (dir, path) = empty_collection();
        Run {
            collection: open(&path).unwrap(),
            clock: StepClock::new(NOW_MS, UtcOffset::UTC, "UTC"),
            notes: Vec::new(),
            cards: Vec::new(),
            revlog: Vec::new(),
            _dir: dir,
        }
    }

    /// `add_note` with `cards` cards, as one operation.
    fn add_note(&mut self, cards: u16) {
        let writer = self.collection.writer();
        let mut scope = writer
            .begin_write(StampMode::Local, &self.clock, Some(JournalOptions::in_memory()))
            .unwrap();
        let id = scope.add_note(&note(1, &["front", "back"])).unwrap();
        self.notes.push(id);
        for ord in 0..cards {
            self.cards.push(scope.add_card(&card(id, ord)).unwrap());
        }
        scope.commit().unwrap();
    }

    /// An answer's review-log row for `#c<n>`, as one operation.
    fn answer(&mut self, n: usize) {
        let writer = self.collection.writer();
        let mut scope = writer
            .begin_write(StampMode::Local, &self.clock, Some(JournalOptions::in_memory()))
            .unwrap();
        let id = scope.add_revlog(&revlog(self.cards[n - 1], 2000)).unwrap();
        scope.commit().unwrap();
        self.revlog.push((n, id.0));
    }

    /// The `ids` observation of these logical refs.
    fn ids(&self, refs: &[&str]) -> Value {
        let mut out = Map::new();
        for r in refs {
            let (kind, n) = r.split_at(2);
            let n: usize = n.parse().unwrap();
            let id = if kind == "#n" {
                self.notes[n - 1].0
            } else {
                self.cards[n - 1].0
            };
            out.insert((*r).to_owned(), json!(id));
        }
        Value::Object(out)
    }

    /// The review-log ids of `#c<n>`, in order.
    fn revlog_ids(&self, n: usize) -> Vec<i64> {
        self.revlog.iter().filter(|(c, _)| *c == n).map(|(_, id)| *id).collect()
    }

    fn set_clock(&self, ms: i64) {
        self.clock.set_now_ms(ms);
    }
}

/// The ids an `ids` observation of the golden lists.
fn golden_ids(step: &Value) -> Value {
    step["ids"].clone()
}

fn golden_revlog_ids(step: &Value, card: &str) -> Vec<i64> {
    step["revlog"][card]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row["id"].as_i64().unwrap())
        .collect()
}

#[test]
fn rows_added_at_one_instant_take_now_then_the_largest_plus_one() {
    let steps = expected_steps("storage/ids/same-instant");
    let mut run = Run::new();
    run.add_note(1);
    run.add_note(1);
    run.add_note(3);
    let all = ["#c1", "#c2", "#c3", "#c4", "#c5", "#n1", "#n2", "#n3"];
    assert_eq!(run.ids(&all), golden_ids(&steps[3]));
}

#[test]
fn a_free_now_is_taken_even_when_larger_ids_exist_and_the_next_takes_the_largest_plus_one() {
    let steps = expected_steps("storage/ids/taken-above-now");
    let mut run = Run::new();
    run.set_clock(NOW_MS + 86_399_000);
    run.add_note(1);
    run.set_clock(NOW_MS);
    run.add_note(1);
    run.add_note(1);
    assert_eq!(
        run.ids(&["#c1", "#c2", "#c3", "#n1", "#n2", "#n3"]),
        golden_ids(&steps[5])
    );
}

#[test]
fn after_the_clock_moves_back_two_days_new_ids_stay_within_a_day_of_now() {
    let steps = expected_steps("storage/ids/clock-back-cap");
    let mut run = Run::new();
    run.set_clock(NOW_MS + 3 * DAY_MS);
    run.add_note(1);
    run.set_clock(NOW_MS + DAY_MS);
    run.add_note(1);
    assert_eq!(run.ids(&["#c1", "#c2", "#n1", "#n2"]), golden_ids(&steps[4]));
    run.add_note(1);
    assert_eq!(run.ids(&["#c3", "#n3"]), golden_ids(&steps[6]));
    // Never a collision, never past the cap.
    let mut all: Vec<i64> = run.notes.iter().map(|n| n.0).collect();
    all.dedup();
    assert_eq!(all.len(), 3);
    assert!(run.notes[2].0 <= NOW_MS + 2 * DAY_MS);
}

#[test]
fn one_millisecond_past_the_cap_takes_the_smallest_free_id_above_now() {
    let steps = expected_steps("storage/ids/cap-boundary");
    let mut run = Run::new();
    run.set_clock(NOW_MS + DAY_MS);
    run.add_note(1);
    run.set_clock(NOW_MS);
    run.add_note(1);
    assert_eq!(run.ids(&["#c1", "#c2", "#n1", "#n2"]), golden_ids(&steps[4]));
    run.add_note(1);
    assert_eq!(run.ids(&["#c3", "#n3"]), golden_ids(&steps[6]));
}

#[test]
fn review_log_rows_at_one_instant_take_now_then_the_largest_plus_one() {
    let steps = expected_steps("storage/ids/revlog-same-instant");
    let mut run = Run::new();
    run.add_note(1);
    run.add_note(1);
    run.answer(1);
    run.answer(2);
    assert_eq!(run.revlog_ids(1), golden_revlog_ids(&steps[2], "#c1"));
    assert_eq!(run.revlog_ids(2), golden_revlog_ids(&steps[2], "#c2"));
}

#[test]
fn review_log_ids_after_the_clock_moves_back_stay_within_a_day_of_now() {
    let steps = expected_steps("storage/ids/revlog-clock-back-cap");
    let mut run = Run::new();
    for _ in 0..3 {
        run.add_note(1);
    }
    run.set_clock(NOW_MS + 3 * DAY_MS);
    run.answer(1);
    run.set_clock(NOW_MS + DAY_MS);
    run.answer(2);
    run.answer(3);
    assert_eq!(run.revlog_ids(1), golden_revlog_ids(&steps[5], "#c1"));
    assert_eq!(run.revlog_ids(2), golden_revlog_ids(&steps[5], "#c2"));
    assert_eq!(run.revlog_ids(3), golden_revlog_ids(&steps[6], "#c3"));
}

/// The golden's `db_exec` rows as `[id, name]` pairs.
fn id_names(step: &Value) -> Vec<(i64, String)> {
    step["db_exec"]["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| (row[0].as_i64().unwrap(), row[1].as_str().unwrap().to_owned()))
        .collect()
}

#[test]
fn decks_presets_and_note_types_follow_the_same_rule() {
    let steps = expected_steps("storage/ids/all-tables");
    let mut run = Run::new();
    let clock = &run.clock;
    let writer = run.collection.writer();
    // A new collection holds the default deck and preset (id 1, never
    // allocated) and stock note types made at the same instant: the golden
    // shows its note types up to now + 5.
    let mut scope = writer.begin_write(StampMode::Raw, clock, None).unwrap();
    let mut default_deck = deck("Default");
    default_deck.id.0 = 1;
    scope.put(&default_deck).unwrap();
    let mut default_preset = preset("Default");
    default_preset.id.0 = 1;
    scope.put(&default_preset).unwrap();
    for i in 0..6 {
        let mut stock = notetype(&format!("Stock {i}"), &["F"], &["T"]);
        stock.id.0 = NOW_MS + i;
        scope.put_notetype(&stock).unwrap();
    }
    scope.commit().unwrap();

    let mut added = Vec::new();
    for table in 0..3 {
        let names: &[&str] = [
            &["Slow", "Fast"][..],
            &["Inbox", "Verbs", "Nouns"],
            &["Alpha", "Beta", "Gamma"],
        ][table];
        let mut rows = Vec::new();
        for name in names {
            let mut scope = writer
                .begin_write(StampMode::Local, clock, Some(JournalOptions::in_memory()))
                .unwrap();
            let id = match table {
                0 => scope.add_deck_config(&preset(name)).unwrap().0,
                1 => scope.add_deck(&deck(name)).unwrap().0,
                _ => {
                    scope
                        .add_notetype(&notetype(name, &["Front", "Back"], &["One"]))
                        .unwrap()
                        .0
                }
            };
            scope.commit().unwrap();
            rows.push((id, (*name).to_owned()));
        }
        added.push(rows);
    }
    for (table, rows) in added.iter_mut().enumerate() {
        rows.sort();
        assert_eq!(*rows, id_names(&steps[table]), "table {table}");
    }
    run.notes.clear();
}
