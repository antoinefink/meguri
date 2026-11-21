//! Browse's card and note bulk actions (`specs/browser/bulk-actions.md` B1,
//! B7.2, B9, B10, B11, B16; `specs/browser/workspace.md` V8.1) on the bulk
//! framework (`specs/browser/bulk-framework.md`), beyond the conformance
//! scenarios: Change Deck's table, refusals, the deck it creates, the
//! remaining steps and the memory states (B7.2 steps 0 to 6), with a card
//! changed between the compute phase and the commit; Unsuspend and Unbury
//! on their subsets only; Flag sets; the browser keys of Set Due Date and
//! Forget; the tag entry's separators; Delete Notes' two counts; B16's
//! `affected` and `considered`; the review session's admission; a notes-mode
//! handle selection; and one undo step per action that gives back every row.

mod common;
mod session_support;

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use common::Harness;
use common::steps::{Dump, dump};
use meguri_engine::browse::search::{SearchEvent, SearchListener, SearchMode, SortDirection, SortSpec};
use meguri_engine::bulk::cards::{self, CardAction};
use meguri_engine::bulk::change_deck::{self, DeckTarget};
use meguri_engine::bulk::notes::{self as note_actions, NoteAction};
use meguri_engine::bulk::{BulkCounts, CardTarget, NoteTarget};
use meguri_engine::decks::{self, Refusal as DeckRefusal};
use meguri_engine::sched::ops::{InvalidDays, Refusal as SchedRefusal};
use meguri_engine::{
    CancelToken, Collection, EngineError, OpResult, OpSource, OpSpec, Progress, ProgressListener, ProgressPhase,
    ProgressSink, UndoScope,
};
use meguri_model::card::{Card, CardQueue, CardType};
use meguri_model::card_data::{CardData, MemoryState};
use meguri_model::deck::Deck;
use meguri_model::ids::{CardId, DeckConfigId, DeckId, NoteId};
use meguri_model::proto::ProtoBlob;
use meguri_model::proto::deck::{DeckKind, DeckKindMessage, FilteredDeck};
use meguri_model::revlog::{RevlogEntry, RevlogKind};
use meguri_model::time::EpochMillis;
use meguri_model::usn::Usn;
use meguri_storage::raw::SqlValue;
use meguri_storage::{Cell, ConfigValue, ReadRows, ReaderRole, Record, StampMode, Table};

// --- Helpers ---------------------------------------------------------------

const DAY_MS: i64 = 86_400_000;

fn counts(affected: u64, considered: u64) -> BulkCounts {
    BulkCounts { affected, considered }
}

fn read(c: &Collection, id: CardId) -> Card {
    c.reader(ReaderRole::Query).unwrap().card(id).unwrap().expect("the card exists")
}

/// A card's row as stored, for whole-row comparisons.
fn cells(c: &Collection, id: CardId) -> Vec<Cell> {
    read(c, id).to_cells()
}

fn deck_rows(c: &Collection) -> Vec<Deck> {
    c.reader(ReaderRole::Query).unwrap().deck_rows().unwrap()
}

fn deck_named(c: &Collection, human: &str) -> Option<Deck> {
    deck_rows(c).into_iter().find(|deck| deck.human_name() == human)
}

fn undo_name(c: &Collection) -> Option<String> {
    c.peek_undo(UndoScope::Global).unwrap().name().map(str::to_owned)
}

fn redo_name(c: &Collection) -> Option<String> {
    c.peek_redo(UndoScope::Global).unwrap().name().map(str::to_owned)
}

/// One undo gives back `before` exactly, and one redo gives back what the
/// action wrote.
fn exact_undo_redo(c: &Collection, before: &Dump, name: &str) {
    let after = dump(c);
    assert_eq!(undo_name(c).as_deref(), Some(name));
    c.undo(UndoScope::Global).unwrap();
    assert_eq!(&dump(c), before, "one undo of {name} restores every row");
    c.redo(UndoScope::Global).unwrap();
    assert_eq!(dump(c), after, "one redo of {name} writes the same rows");
}

/// The stored `data` text of a card, as the column holds it.
fn data_text(c: &Collection, id: CardId) -> String {
    let rows = c
        .reader(ReaderRole::Query)
        .unwrap()
        .execute_raw(&format!("SELECT data FROM cards WHERE id = {}", id.0), &[])
        .unwrap()
        .rows;
    match &rows[0][0] {
        SqlValue::Text(text) => text.clone(),
        other => panic!("data is not text: {other:?}"),
    }
}

/// One raw statement on the writer, outside any op (a `data` text no
/// typed write produces).
fn raw_sql(h: &Harness, sql: String) {
    h.collection
        .submit_maintenance(
            meguri_engine::executor::catalog::ANALYZE,
            Duration::from_millis(10),
            move |w| {
                w.execute_raw(&sql, &[])?;
                Ok(())
            },
        )
        .wait()
        .unwrap();
}

fn set_fsrs(c: &Collection, on: bool) {
    c.run(
        OpSpec::named("set_config", OpSource::Rollover).stamp(StampMode::Raw),
        move |ctx| Ok(ctx.scope().set_config("fsrs", &ConfigValue::Bool(on))?),
    )
    .unwrap();
}

/// Moves the clock a minute and marks every card sent (usn 0), so a write
/// shows a new `mod` and `usn` -1, and clears the history.
fn settle(c: &Collection, h: &Harness) {
    h.clock.step_ms(60_000);
    c.run(
        OpSpec::named("settle", OpSource::Rollover).stamp(StampMode::Raw),
        |ctx| {
            let scope = ctx.scope();
            for id in scope.ids_after(Table::Cards, None, usize::MAX)? {
                let mut card = scope.card(CardId(id))?.unwrap();
                card.usn = Usn(0);
                scope.put(&card)?;
            }
            Ok(())
        },
    )
    .unwrap();
    c.clear_undo(meguri_engine::ClearCause::CheckDatabase).unwrap();
}

fn change(c: &Collection, cards: &[CardId], deck: DeckTarget) -> Result<OpResult<BulkCounts>, EngineError> {
    change_deck::run(
        c,
        &CardTarget::Cards(cards.to_vec()),
        &deck,
        CancelToken::new(),
        ProgressSink::new(),
    )
}

fn act(c: &Collection, cards: &[CardId], action: CardAction) -> Result<OpResult<BulkCounts>, EngineError> {
    cards::run(
        c,
        &CardTarget::Cards(cards.to_vec()),
        &action,
        CancelToken::new(),
        ProgressSink::new(),
    )
}

fn note_act(c: &Collection, notes: &[NoteId], action: NoteAction) -> Result<OpResult<note_actions::NoteCounts>, EngineError> {
    note_actions::run(
        c,
        &NoteTarget::Notes(notes.to_vec()),
        &action,
        CancelToken::new(),
        ProgressSink::new(),
    )
}

fn normal_deck(name: &str, preset: DeckConfigId) -> Deck {
    let mut deck = decks::new_deck(name.to_owned());
    if let DeckKind::Normal(normal) = &mut deck.kind.kind {
        normal.config_id = preset.0;
    }
    deck
}

fn filtered_deck(name: &str) -> Deck {
    let mut deck = decks::new_deck(name.to_owned());
    deck.kind = ProtoBlob::new(DeckKindMessage::new(DeckKind::Filtered(FilteredDeck::default())));
    deck
}

/// A card of a note of its own (`front` keeps notes and their guids apart).
fn add_card(scope: &mut meguri_storage::WriteScope<'_>, front: &str, deck: DeckId, edit: impl FnOnce(&mut Card)) -> CardId {
    let nid = scope.add_note(&common::note(&[front, "back"])).unwrap();
    let mut card = common::card(nid, 0);
    card.deck_id = deck;
    edit(&mut card);
    scope.add_card(&card).unwrap()
}

/// A review-log row of `card` at `at`: graded Good, or for `Manual` a
/// set-due-date row without a button.
fn graded(scope: &mut meguri_storage::WriteScope<'_>, card: CardId, kind: RevlogKind, interval: i32, at: i64) {
    let entry = RevlogEntry {
        kind,
        interval,
        button: if kind == RevlogKind::Manual { 0 } else { 3 },
        ..common::revlog(card)
    };
    scope.add_answer_revlog(&entry, EpochMillis(at)).unwrap();
}

/// The decks and presets of most tests: `Home` and `Other` on preset 1
/// (learning steps 1 10 60, relearning 10), `Target` and `Target::Sub` on
/// `Strict` (learning 5 120, relearning 5 30 120, desired retention 0.85),
/// and the filtered deck `Cram`.
struct World {
    h: Harness,
    home: DeckId,
    other: DeckId,
    target: DeckId,
    sub: DeckId,
    cram: DeckId,
}

/// The deck ids, for the ops that build cards.
#[derive(Clone, Copy)]
struct Decks {
    home: DeckId,
    other: DeckId,
    target: DeckId,
    sub: DeckId,
    cram: DeckId,
}

fn world() -> World {
    let h = common::fresh();
    let (home, other, target, sub, cram) = h
        .collection
        .run(OpSpec::named("u87 decks", OpSource::Maintenance), |ctx| {
            let s = ctx.scope();
            let mut basic = s.deck_config(DeckConfigId(1))?.unwrap();
            basic.config.learn_steps = vec![1.0, 10.0, 60.0];
            basic.config.relearn_steps = vec![10.0];
            s.put(&basic)?;
            let mut strict = basic.clone();
            strict.name = "Strict".into();
            strict.config.learn_steps = vec![5.0, 120.0];
            strict.config.relearn_steps = vec![5.0, 30.0, 120.0];
            strict.config.desired_retention = 0.85;
            let strict = s.add_deck_config(&strict)?;
            let home = s.add_deck(&normal_deck("Home", DeckConfigId(1)))?;
            let other = s.add_deck(&normal_deck("Other", DeckConfigId(1)))?;
            let target = s.add_deck(&normal_deck("Target", strict))?;
            let sub = s.add_deck(&normal_deck("Target\u{1f}Sub", strict))?;
            let cram = s.add_deck(&filtered_deck("Cram"))?;
            Ok((home, other, target, sub, cram))
        })
        .unwrap()
        .value;
    World {
        h,
        home,
        other,
        target,
        sub,
        cram,
    }
}

impl World {
    fn decks(&self) -> Decks {
        Decks {
            home: self.home,
            other: self.other,
            target: self.target,
            sub: self.sub,
            cram: self.cram,
        }
    }

    /// Cards `build` adds in one op; then every card is marked sent and the
    /// history cleared.
    fn cards(
        &self,
        build: impl FnOnce(&mut meguri_storage::WriteScope<'_>, Decks) -> Vec<CardId> + Send + 'static,
    ) -> Vec<CardId> {
        let decks = self.decks();
        let ids = self
            .h
            .collection
            .run(OpSpec::named("u87 cards", OpSource::Maintenance), move |ctx| {
                Ok(build(ctx.scope(), decks))
            })
            .unwrap()
            .value;
        settle(&self.h.collection, &self.h);
        ids
    }
}

// --- B7.2: Change Deck --------------------------------------------------------

/// B7.2's table with FSRS off: a new card moved and stamped; a card already
/// in the target untouched and not counted; two filtered cards sent home
/// first (`due` from `odue`, queue from the type, `odid` and `odue` 0), the
/// one whose home deck is the target counted too; a suspended card kept so;
/// a learning card's memory keys removed, `pos` and `lrt` kept; a card
/// deleted earlier skipped. One "Change Deck" step that undo and redo give
/// back exactly.
#[test]
fn b7_2_table_moves_skips_and_one_step() {
    let w = world();
    let c = &w.h.collection;
    let ids = w.cards(move |s, w| {
        let new = add_card(s, "new", w.home, |_| {});
        let there = add_card(s, "there", w.target, |_| {});
        let review = |card: &mut Card| {
            card.card_type = CardType::Review;
            card.queue = CardQueue::Review;
            card.interval = 12;
            card.due = -50;
        };
        let cram_home = add_card(s, "cram home", w.cram, |card| {
            review(card);
            card.original_deck_id = w.home;
            card.original_due = 6;
        });
        let cram_target = add_card(s, "cram target", w.cram, |card| {
            review(card);
            card.original_deck_id = w.target;
            card.original_due = 9;
        });
        let suspended = add_card(s, "suspended", w.other, |card| {
            review(card);
            card.due = 40;
            card.queue = CardQueue::Suspended;
        });
        let learning = add_card(s, "learning", w.home, |card| {
            card.card_type = CardType::Learning;
            card.queue = CardQueue::InterdayLearning;
            card.due = 30;
            card.remaining_steps = 1;
            card.data = CardData::parse(r#"{"pos":7,"s":4.5,"d":5.5,"dr":0.9,"decay":0.1542,"lrt":1894000000}"#);
        });
        let gone = add_card(s, "gone", w.home, |_| {});
        s.remove_note_and_cards(s.card(gone).unwrap().unwrap().note_id).unwrap();
        vec![new, there, cram_home, cram_target, suspended, learning, gone]
    });
    set_fsrs(c, false);
    let [new, there, cram_home, cram_target, suspended, learning, gone] = ids[..] else {
        unreachable!()
    };
    let before = dump(c);
    let there_before = cells(c, there);
    let done = change(c, &ids, DeckTarget::Deck(w.target)).unwrap();
    assert_eq!(done.value, counts(5, 7), "B7.2 step 5: the moved cards; B16: every listed id");
    assert_eq!(done.undo.map(|u| u.name), Some("Change Deck"));
    let moved = |id| {
        let card = read(c, id);
        assert_eq!(card.deck_id, w.target, "{id:?}");
        assert_eq!(card.usn, Usn(-1), "{id:?} stamped");
        card
    };
    moved(new);
    assert_eq!(cells(c, there), there_before, "already in the target: untouched");
    let home = moved(cram_home);
    assert_eq!((home.due, home.queue, home.original_deck_id, home.original_due), (6, CardQueue::Review, DeckId(0), 0));
    let target = moved(cram_target);
    assert_eq!((target.due, target.queue, target.original_deck_id), (9, CardQueue::Review, DeckId(0)));
    let kept = moved(suspended);
    assert_eq!((kept.queue, kept.due, kept.interval), (CardQueue::Suspended, 40, 12), "moving never reschedules");
    assert_eq!(data_text(c, learning), r#"{"pos":7,"lrt":1894000000}"#);
    assert!(c.reader(ReaderRole::Query).unwrap().card(gone).unwrap().is_none());
    exact_undo_redo(c, &before, "Change Deck");
}

/// B7.2 step 1's refusals in order, each writing nothing and leaving both
/// histories as they were: an empty or repeated list, step 0's refusals
/// (`filtered_parent`, `too_deep`), a deck id that names no deck, a
/// filtered target by id and by typed name.
#[test]
fn b7_2_refusals_write_nothing_and_keep_both_histories() {
    let w = world();
    let c = &w.h.collection;
    let ids = w.cards(move |s, w| vec![add_card(s, "a", w.home, |_| {}), add_card(s, "b", w.home, |_| {})]);
    set_fsrs(c, false);
    // A step to undo and one to redo, so both histories can be compared.
    change(c, &ids[..1], DeckTarget::Deck(w.other)).unwrap();
    change(c, &ids[1..], DeckTarget::Deck(w.other)).unwrap();
    c.undo(UndoScope::Global).unwrap();
    let before = dump(c);
    let histories = || (undo_name(c), redo_name(c));
    let kept = histories();
    assert_eq!(kept, (Some("Change Deck".into()), Some("Change Deck".into())));
    let refused = |cards: &[CardId], deck: DeckTarget| change(c, cards, deck).unwrap_err();
    assert!(matches!(refused(&[], DeckTarget::Deck(w.target)), EngineError::InvalidArgument { .. }));
    assert!(matches!(
        refused(&[ids[0], ids[0]], DeckTarget::Deck(w.target)),
        EngineError::InvalidArgument { .. }
    ));
    // The list is checked before the deck.
    assert!(matches!(refused(&[ids[0], ids[0]], DeckTarget::Deck(DeckId(-9))), EngineError::InvalidArgument { .. }));
    assert_eq!(
        refused(&ids, DeckTarget::NewDeck("Cram::Inside".into())),
        EngineError::DeckRefused(DeckRefusal::FilteredParent)
    );
    let deep = (1..=12).map(|i| i.to_string()).collect::<Vec<_>>().join("::");
    assert_eq!(refused(&ids, DeckTarget::NewDeck(deep)), EngineError::DeckRefused(DeckRefusal::TooDeep));
    assert!(matches!(refused(&ids, DeckTarget::Deck(DeckId(-9))), EngineError::NotFound { .. }));
    assert_eq!(refused(&ids, DeckTarget::Deck(w.cram)), EngineError::DeckRefused(DeckRefusal::FilteredDeck));
    assert_eq!(
        refused(&ids, DeckTarget::NewDeck("cram".into())),
        EngineError::DeckRefused(DeckRefusal::FilteredDeck),
        "a typed name of an existing filtered deck"
    );
    assert_eq!(dump(c), before);
    assert_eq!(histories(), kept);
}

/// B7.2 step 0 and step 6: a typed name creates the deck and its missing
/// ancestors inside the one "Change Deck" step, and undo removes them with
/// the move; a typed name of an existing deck (another case) creates none;
/// a blank name is `blank`; with no card to move, the created deck is the
/// step alone.
#[test]
fn b7_2_step_0_creates_the_deck_inside_the_step() {
    let w = world();
    let c = &w.h.collection;
    let ids = w.cards(move |s, w| (0..3).map(|i| add_card(s, &format!("n{i}"), w.home, |_| {})).collect());
    set_fsrs(c, false);
    let before = dump(c);
    let decks_before = deck_rows(c).len();
    let done = change(c, &ids, DeckTarget::NewDeck("Zed::Kid".into())).unwrap();
    assert_eq!(done.value, counts(3, 3));
    let kid = deck_named(c, "Zed::Kid").expect("the new deck");
    assert!(deck_named(c, "Zed").is_some(), "its missing ancestor");
    assert_eq!(deck_rows(c).len(), decks_before + 2);
    assert!(ids.iter().all(|id| read(c, *id).deck_id == kid.id));
    assert!(matches!(&kid.kind.kind, DeckKind::Normal(normal) if normal.config_id == 1), "D4: preset 1");
    exact_undo_redo(c, &before, "Change Deck");
    c.undo(UndoScope::Global).unwrap();
    assert!(deck_named(c, "Zed").is_none() && deck_named(c, "Zed::Kid").is_none(), "undo removes both decks");

    // An existing name in another case: no deck created.
    let done = change(c, &ids, DeckTarget::NewDeck("TARGET".into())).unwrap();
    assert_eq!(done.value, counts(3, 3));
    assert_eq!(deck_rows(c).len(), decks_before);
    assert!(ids.iter().all(|id| read(c, *id).deck_id == w.target));

    // A blank name is the deck `blank` (D1), created once then found.
    change(c, &ids[..1], DeckTarget::NewDeck(String::new())).unwrap();
    let blank = deck_named(c, "blank").expect("D1: blank");
    change(c, &ids[1..2], DeckTarget::NewDeck("  ".into())).unwrap();
    assert_eq!(read(c, ids[1]).deck_id, blank.id);
    assert_eq!(deck_rows(c).len(), decks_before + 1);

    // Nothing to move: the created deck alone is the step.
    let gone = w.cards(move |s, w| {
        let id = add_card(s, "gone", w.home, |_| {});
        s.remove_note_and_cards(s.card(id).unwrap().unwrap().note_id).unwrap();
        vec![id]
    });
    let before = dump(c);
    let done = change(c, &gone, DeckTarget::NewDeck("Lone".into())).unwrap();
    assert_eq!(done.value, counts(0, 1));
    assert!(deck_named(c, "Lone").is_some());
    exact_undo_redo(c, &before, "Change Deck");
    // A count of 0 with no deck created records no step.
    let before = dump(c);
    let undo = undo_name(c);
    let done = change(c, &ids[..1], DeckTarget::Deck(read(c, ids[0]).deck_id)).unwrap();
    assert_eq!(done.value, counts(0, 1));
    assert!(done.undo.is_none());
    assert_eq!(dump(c), before);
    assert_eq!(undo_name(c), undo);
}

/// B7.2 step 3.1 (`sched/step-remap` R4): learning and relearning counts
/// translated from the home deck's preset to the target's, a filtered
/// card's home preset read; a review card's `left` kept; equal presets
/// (Home to Other) still translate, which changes nothing for increasing
/// lists.
#[test]
fn b7_2_step_3_1_translates_the_remaining_steps() {
    let w = world();
    let c = &w.h.collection;
    let learning = |left: u32| {
        move |card: &mut Card| {
            card.card_type = CardType::Learning;
            card.queue = CardQueue::Learning;
            card.due = 1_894_701_000;
            card.remaining_steps = left;
        }
    };
    let ids = w.cards(move |s, w| {
        vec![
            // 1 10 60 → 5 120: R2's first rows.
            add_card(s, "l3", w.home, learning(3)),
            add_card(s, "l2", w.home, learning(2)),
            add_card(s, "l0", w.home, learning(0)),
            add_card(s, "l1002", w.home, learning(1002)),
            // Relearning 10 → 5 30 120.
            add_card(s, "r1", w.home, |card| {
                learning(1)(card);
                card.card_type = CardType::Relearning;
            }),
            // In Cram, home Home: its home preset gives the old list.
            add_card(s, "f3", w.cram, |card| {
                learning(3)(card);
                card.original_deck_id = w.home;
                card.original_due = 1_894_701_000;
            }),
            add_card(s, "review", w.home, |card| {
                card.card_type = CardType::Review;
                card.queue = CardQueue::Review;
                card.remaining_steps = 3;
            }),
        ]
    });
    set_fsrs(c, false);
    change(c, &ids, DeckTarget::Deck(w.target)).unwrap();
    let left: Vec<u32> = ids.iter().map(|id| read(c, *id).remaining_steps).collect();
    assert_eq!(left, [2, 2, 1, 1002, 3, 2, 3]);
    // Same preset: 1 10 60 → 1 10 60 keeps every count.
    let more = w.cards(move |s, w| vec![add_card(s, "same", w.home, learning(2))]);
    change(c, &more, DeckTarget::Deck(w.other)).unwrap();
    assert_eq!(read(c, more[0]).remaining_steps, 2);
}

/// B7.2 step 4 with FSRS on (`sched/memory-state` M7.2): the moved cards
/// and the target deck's cards whose stability reads as NULL get the target
/// preset's memory states with the target's desired retention; a target
/// card holding `s` is not written though its `dr` differs; one with `d`
/// and no `s` is computed, one with `s` and no `d` keeps its row as stored;
/// a card with only a set-due-date row gets `dr` and `decay` only; new
/// cards, one with a review-log row, keep their rows; subdeck and other
/// cards are untouched; a filtered card whose home deck is the target is
/// computed and stays in its filtered deck. Only the moved cards count.
#[test]
fn b7_2_step_4_memory_states() {
    let w = world();
    let c = &w.h.collection;
    let now = common::NOW_MS;
    let review = |card: &mut Card| {
        card.card_type = CardType::Review;
        card.queue = CardQueue::Review;
        card.interval = 10;
        card.due = 20;
    };
    let ids = w.cards(move |s, w| {
        let history = |s: &mut meguri_storage::WriteScope<'_>, id: CardId| {
            graded(s, id, RevlogKind::Learning, -600, now - 20 * DAY_MS);
            graded(s, id, RevlogKind::Review, 10, now - 10 * DAY_MS);
        };
        let moved = add_card(s, "moved", w.home, |card| {
            review(card);
            card.data = CardData::parse(r#"{"s":40.0,"d":3.0,"dr":0.9,"decay":0.1542}"#);
        });
        history(s, moved);
        let stateless = add_card(s, "stateless", w.target, review);
        history(s, stateless);
        let holds = add_card(s, "holds", w.target, |card| {
            review(card);
            card.data = CardData::parse(r#"{"s":12.0,"d":4.0,"dr":0.7,"decay":0.2}"#);
        });
        history(s, holds);
        let d_only = add_card(s, "d only", w.target, review);
        history(s, d_only);
        let s_only = add_card(s, "s only", w.target, review);
        history(s, s_only);
        let manual = add_card(s, "manual", w.target, review);
        graded(s, manual, RevlogKind::Manual, 10, now - DAY_MS);
        let new_logged = add_card(s, "new logged", w.target, |_| {});
        graded(s, new_logged, RevlogKind::Manual, 0, now - DAY_MS);
        let sub = add_card(s, "sub", w.sub, review);
        history(s, sub);
        let other = add_card(s, "other", w.other, review);
        history(s, other);
        let filtered = add_card(s, "filtered", w.cram, |card| {
            review(card);
            card.original_deck_id = w.target;
            card.original_due = 20;
            card.due = -10;
        });
        history(s, filtered);
        vec![moved, stateless, holds, d_only, s_only, manual, new_logged, sub, other, filtered]
    });
    let [moved, stateless, holds, d_only, s_only, manual, new_logged, sub, other, filtered] = ids[..] else {
        unreachable!()
    };
    // Data texts no typed write produces (`search/card-extractors` X4).
    raw_sql(&w.h, format!(r#"UPDATE cards SET data = '{{"d":4.0}}' WHERE id = {}"#, d_only.0));
    raw_sql(&w.h, format!(r#"UPDATE cards SET data = '{{"s":3.0}}' WHERE id = {}"#, s_only.0));
    set_fsrs(c, true);
    c.clear_undo(meguri_engine::ClearCause::CheckDatabase).unwrap();
    let rows_before: Vec<Vec<Cell>> = ids.iter().map(|id| cells(c, *id)).collect();
    let texts_before: Vec<String> = ids.iter().map(|id| data_text(c, *id)).collect();
    let before = dump(c);
    let done = change(c, &[moved], DeckTarget::Deck(w.target)).unwrap();
    assert_eq!(done.value, counts(1, 1), "only the moved card counts");
    let computed = |id: CardId| {
        let card = read(c, id);
        assert_eq!(card.data.desired_retention, Some(0.85), "{id:?}: the target's desired retention");
        assert!(card.data.decay.is_some(), "{id:?}");
        assert!(card.data.memory_state.is_some(), "{id:?}: a memory state from its history");
        assert_eq!(card.usn, Usn(-1), "{id:?} written");
        card
    };
    let m = computed(moved);
    assert_ne!(m.data.memory_state, Some(MemoryState { stability: 40.0, difficulty: 3.0 }));
    assert_eq!((m.due, m.interval), (20, 10), "never rescheduled");
    computed(stateless);
    computed(d_only);
    let f = computed(filtered);
    assert_eq!(f.deck_id, w.cram, "stays in its filtered deck");
    let only_dr = read(c, manual);
    assert_eq!((only_dr.data.memory_state, only_dr.data.desired_retention), (None, Some(0.85)));
    assert!(only_dr.data.decay.is_some());
    for (index, id) in ids.iter().enumerate() {
        if [holds, s_only, new_logged, sub, other].contains(id) {
            assert_eq!(cells(c, *id), rows_before[index], "{id:?} keeps its row");
            assert_eq!(data_text(c, *id), texts_before[index], "{id:?} keeps its data text");
        }
    }
    exact_undo_redo(c, &before, "Change Deck");
    // A move of cards already in the target: count 0, nothing computed.
    c.undo(UndoScope::Global).unwrap();
    let before = dump(c);
    let done = change(c, &[stateless], DeckTarget::Deck(w.target)).unwrap();
    assert_eq!(done.value, counts(0, 1));
    assert_eq!(dump(c), before, "nothing computed when nothing moved");
}

/// Change Deck with `edit` run once by another writer after the compute
/// phase's snapshot was taken; returns the result and how many commits it
/// took (2: the first was stale and the action planned again).
fn change_with_edit(
    c: &Collection,
    cards: &[CardId],
    deck: DeckTarget,
    edit: impl Fn(&Collection) + Send + Sync + 'static,
) -> (OpResult<BulkCounts>, u64) {
    struct Listener<F: Fn(&Progress) + Send + Sync>(F);
    impl<F: Fn(&Progress) + Send + Sync> ProgressListener for Listener<F> {
        fn on_progress(&self, progress: &Progress) {
            (self.0)(progress)
        }
    }
    static NOW: AtomicU64 = AtomicU64::new(0);
    let progress = ProgressSink::with_clock(|| Duration::from_secs(NOW.fetch_add(1, Ordering::SeqCst)));
    let edited = Arc::new(AtomicBool::new(false));
    let commits = Arc::new(AtomicU64::new(0));
    let (collection, flag, count) = (c.clone(), Arc::clone(&edited), Arc::clone(&commits));
    progress.set_listener(Arc::new(Listener(move |p: &Progress| {
        if p.phase == ProgressPhase::Committing {
            count.fetch_add(1, Ordering::SeqCst);
        }
        if p.phase == ProgressPhase::Planning
            && p.completed == 0
            && p.total.is_some()
            && !flag.swap(true, Ordering::SeqCst)
        {
            edit(&collection);
        }
    })));
    let done = change_deck::run(c, &CardTarget::Cards(cards.to_vec()), &deck, CancelToken::new(), progress).unwrap();
    assert!(edited.load(Ordering::SeqCst), "the edit ran");
    (done, commits.load(Ordering::SeqCst))
}

/// Two review cards with a learning and a review row each, holding a
/// memory state: one in Home, one in Target.
fn two_with_states(w: &World) -> (CardId, CardId) {
    let now = common::NOW_MS;
    let ids = w.cards(move |s, w| {
        let review = |card: &mut Card| {
            card.card_type = CardType::Review;
            card.queue = CardQueue::Review;
            card.interval = 10;
            card.due = 20;
            card.data = CardData::parse(r#"{"s":12.0,"d":4.0,"dr":0.9,"decay":0.1542}"#);
        };
        let moved = add_card(s, "moved", w.home, review);
        let stays = add_card(s, "stays", w.target, review);
        for id in [moved, stays] {
            graded(s, id, RevlogKind::Learning, -600, now - 20 * DAY_MS);
            graded(s, id, RevlogKind::Review, 10, now - 10 * DAY_MS);
        }
        vec![moved, stays]
    });
    set_fsrs(&w.h.collection, true);
    (ids[0], ids[1])
}

/// K3 and B7.2 step 4: a target card whose `s` another writer removes
/// between the compute phase and the commit joins the set, so the commit
/// is stale and the action plans again; a moved card edited meanwhile keeps
/// the edit. Nothing the compute phase read is written over.
#[test]
fn b7_2_a_card_changed_between_the_phases_replans() {
    let w = world();
    let c = &w.h.collection;
    let (moved, stays) = two_with_states(&w);
    let (done, commits) = change_with_edit(c, &[moved], DeckTarget::Deck(w.target), move |collection| {
        collection
            .run(OpSpec::named("another writer", OpSource::UserUndoable), move |ctx| {
                let mut card = ctx.scope().card(stays)?.unwrap();
                card.data.memory_state = None;
                ctx.scope().put(&card)?;
                let mut card = ctx.scope().card(moved)?.unwrap();
                card.interval = 11;
                ctx.scope().put(&card)?;
                Ok(())
            })
            .unwrap();
    });
    assert_eq!(commits, 2, "the first commit was stale: planned again");
    assert_eq!(done.value, counts(1, 1));
    assert_eq!(read(c, moved).interval, 11, "the edit stands");
    assert_eq!(read(c, moved).deck_id, w.target);
    let joined = read(c, stays);
    assert!(joined.data.memory_state.is_some(), "the card that lost its state meanwhile is computed");
    assert_eq!(joined.data.desired_retention, Some(0.85));
}

/// B7.2 step 4: a review-log row added to a moved card between the phases,
/// without touching its card row, makes the commit stale: the memory state
/// is computed from the history that holds it.
#[test]
fn b7_2_a_review_row_added_between_the_phases_replans() {
    let w = world();
    let c = &w.h.collection;
    let (moved, _) = two_with_states(&w);
    let (done, commits) = change_with_edit(c, &[moved], DeckTarget::Deck(w.target), move |collection| {
        collection
            .run(OpSpec::named("a review row alone", OpSource::Maintenance), move |ctx| {
                graded(ctx.scope(), moved, RevlogKind::Review, 30, common::NOW_MS - DAY_MS);
                Ok(())
            })
            .unwrap();
    });
    assert_eq!(commits, 2, "the first commit was stale: planned again");
    assert_eq!(done.value, counts(1, 1));
    assert!(read(c, moved).data.memory_state.is_some());
}

/// B7.2 step 0: a typed name resolves to the deck that bears it when the
/// commit runs. When the deck it named is renamed between the phases, the
/// action plans again, and the name then makes a new deck.
#[test]
fn b7_2_a_typed_name_renamed_between_the_phases_replans() {
    let w = world();
    let c = &w.h.collection;
    let (moved, _) = two_with_states(&w);
    let target = w.target;
    let (done, commits) = change_with_edit(c, &[moved], DeckTarget::NewDeck("target".into()), move |collection| {
        // A plain op: the deck rename op plans on the maintenance reader,
        // which the compute phase holds now.
        collection
            .run(OpSpec::named("rename elsewhere", OpSource::UserUndoable), move |ctx| {
                decks::rename_in(ctx.scope(), target, "Elsewhere")?;
                Ok(())
            })
            .unwrap();
    });
    assert_eq!(commits, 2, "the first commit was stale: planned again");
    assert_eq!(done.value, counts(1, 1));
    let named = deck_named(c, "target").expect("a new deck of the typed name");
    assert_ne!(named.id, target);
    assert_eq!(read(c, moved).deck_id, named.id);
}

/// K4: a cancel before the commit writes nothing and records no step.
#[test]
fn b7_2_a_cancel_before_the_commit_writes_nothing() {
    let w = world();
    let c = &w.h.collection;
    let ids = w.cards(move |s, w| vec![add_card(s, "a", w.home, |_| {})]);
    let before = dump(c);
    let token = CancelToken::new();
    token.cancel();
    let e = change_deck::run(
        c,
        &CardTarget::Cards(ids),
        &DeckTarget::NewDeck("Never".into()),
        token,
        ProgressSink::new(),
    )
    .unwrap_err();
    assert_eq!(e, EngineError::Cancelled);
    assert_eq!(dump(c), before);
    assert_eq!(undo_name(c), None);
}

// --- V8.1: the other card actions ----------------------------------------------

/// The queues of most card-action tests: suspended, buried by the user,
/// buried by the scheduler, and a plain review card.
fn queues(w: &World) -> Vec<CardId> {
    w.cards(move |s, w| {
        [
            CardQueue::Suspended,
            CardQueue::BuriedByUser,
            CardQueue::BuriedByScheduler,
            CardQueue::Review,
        ]
        .into_iter()
        .enumerate()
        .map(|(i, queue)| {
            add_card(s, &format!("q{i}"), w.home, |card| {
                card.card_type = CardType::Review;
                card.queue = queue;
                card.interval = 5;
                card.due = 30;
            })
        })
        .collect()
    })
}

/// V8.1: Unsuspend is M4 on the suspended targets only, Unbury on the
/// buried ones only; each is one "Unbury/Unsuspend" step.
#[test]
fn v8_1_unsuspend_and_unbury_act_on_their_subsets_only() {
    let w = world();
    let c = &w.h.collection;
    let ids = queues(&w);
    let queue_of = |id| read(c, id).queue;
    let before = dump(c);
    let done = act(c, &ids, CardAction::Unsuspend).unwrap();
    assert_eq!(done.value, counts(1, 4));
    assert_eq!(
        ids.iter().map(|id| queue_of(*id)).collect::<Vec<_>>(),
        [
            CardQueue::Review,
            CardQueue::BuriedByUser,
            CardQueue::BuriedByScheduler,
            CardQueue::Review
        ]
    );
    exact_undo_redo(c, &before, "Unbury/Unsuspend");
    c.undo(UndoScope::Global).unwrap();
    let done = act(c, &ids, CardAction::Unbury).unwrap();
    assert_eq!(done.value, counts(2, 4));
    assert_eq!(
        ids.iter().map(|id| queue_of(*id)).collect::<Vec<_>>(),
        [CardQueue::Suspended, CardQueue::Review, CardQueue::Review, CardQueue::Review]
    );
    exact_undo_redo(c, &before, "Unbury/Unsuspend");
    c.undo(UndoScope::Global).unwrap();
    // No suspended target: nothing written, no step.
    let done = act(c, &ids[1..], CardAction::Unsuspend).unwrap();
    assert_eq!(done.value, counts(0, 3));
    assert!(done.undo.is_none());
    assert_eq!(dump(c), before);
}

/// V8.1: Suspend (M3) and Bury (M2) on every target card, as their own
/// steps; already suspended targets stay, and Bury leaves a suspended card.
#[test]
fn v8_1_suspend_and_bury() {
    let w = world();
    let c = &w.h.collection;
    let ids = queues(&w);
    let before = dump(c);
    let done = act(c, &ids, CardAction::Suspend).unwrap();
    assert_eq!(done.value, counts(3, 4));
    assert!(ids.iter().all(|id| read(c, *id).queue == CardQueue::Suspended));
    exact_undo_redo(c, &before, "Suspend");
    c.undo(UndoScope::Global).unwrap();
    let done = act(c, &ids, CardAction::Bury).unwrap();
    assert_eq!(done.value, counts(2, 4));
    assert_eq!(read(c, ids[0]).queue, CardQueue::Suspended);
    assert!(ids[1..].iter().all(|id| read(c, *id).queue == CardQueue::BuriedByUser));
    exact_undo_redo(c, &before, "Bury");
}

/// V8.1, F2: Flag sets the chosen flag on every target (cards that hold it
/// are not written), No Flag clears; the card's `mod` stays; a flag above
/// 7 is refused. One "Set Flag" step each.
#[test]
fn v8_1_flag_sets_and_no_flag_clears() {
    let w = world();
    let c = &w.h.collection;
    let ids = w.cards(move |s, w| {
        [0u8, 2, 5]
            .into_iter()
            .map(|flag| add_card(s, &format!("f{flag}"), w.home, |card| card.flags = flag | 0b1000))
            .collect()
    });
    let mods: Vec<_> = ids.iter().map(|id| read(c, *id).mtime).collect();
    let before = dump(c);
    let done = act(c, &ids, CardAction::SetFlag(2)).unwrap();
    assert_eq!(done.value, counts(2, 3));
    for (id, mtime) in ids.iter().zip(&mods) {
        let card = read(c, *id);
        assert_eq!(card.flags, 2 | 0b1000, "the other bits kept");
        assert_eq!(card.mtime, *mtime, "F2: mod kept");
    }
    assert_eq!(read(c, ids[0]).usn, Usn(-1));
    assert_eq!(read(c, ids[1]).usn, Usn(0), "a card that holds the flag is not written");
    exact_undo_redo(c, &before, "Set Flag");
    let done = act(c, &ids, CardAction::SetFlag(0)).unwrap();
    assert_eq!(done.value, counts(3, 3));
    assert!(ids.iter().all(|id| read(c, *id).flags == 0b1000));
    let before = dump(c);
    assert!(matches!(act(c, &ids, CardAction::SetFlag(8)), Err(EngineError::InvalidArgument { .. })));
    assert_eq!(dump(c), before);
}

fn config_text(c: &Collection, key: &str) -> Option<String> {
    c.reader(ReaderRole::Query)
        .unwrap()
        .config_value(key)
        .unwrap()
        .map(|value| value.compact())
}

/// B10: Set Due Date stores the text as typed in `setDueBrowser`, and
/// Forget its two choices in `restorePositionBrowser` and
/// `resetCountsBrowser`, inside their own steps; text that does not parse
/// is refused before anything is written.
#[test]
fn b10_set_due_date_and_forget_store_the_browser_keys_in_their_steps() {
    let w = world();
    let c = &w.h.collection;
    let ids = queues(&w);
    let before = dump(c);
    let stored = config_text(c, "setDueBrowser");
    let done = act(c, &ids[3..], CardAction::SetDueDate { days: "3-7!".into() }).unwrap();
    assert_eq!(done.value, counts(1, 1));
    assert_eq!(config_text(c, "setDueBrowser").as_deref(), Some(r#""3-7!""#));
    exact_undo_redo(c, &before, "Set Due Date");
    c.undo(UndoScope::Global).unwrap();
    assert_eq!(config_text(c, "setDueBrowser"), stored, "undo restores the key with the step");
    assert_eq!(
        act(c, &ids, CardAction::SetDueDate { days: "x".into() }).unwrap_err(),
        EngineError::SchedOp(SchedRefusal::InvalidDays(InvalidDays::Malformed))
    );
    assert_eq!(dump(c), before);
    let done = act(
        c,
        &ids,
        CardAction::Forget {
            restore_position: false,
            reset_counts: true,
        },
    )
    .unwrap();
    assert_eq!(done.value, counts(4, 4));
    assert_eq!(config_text(c, "restorePositionBrowser").as_deref(), Some("false"));
    assert_eq!(config_text(c, "resetCountsBrowser").as_deref(), Some("true"));
    assert!(ids.iter().all(|id| read(c, *id).card_type == CardType::New));
    exact_undo_redo(c, &before, "Reset Card");
}

/// B16: `considered` is every target id B1 gave, deleted ones included;
/// `affected` the rows written. A repeated or empty list is refused.
#[test]
fn b16_counts_include_deleted_ids_and_lists_are_checked() {
    let w = world();
    let c = &w.h.collection;
    let mut ids = queues(&w);
    let gone = w.cards(move |s, w| {
        let id = add_card(s, "gone", w.home, |_| {});
        s.remove_note_and_cards(s.card(id).unwrap().unwrap().note_id).unwrap();
        vec![id]
    });
    ids.extend(gone);
    assert_eq!(act(c, &ids, CardAction::SetFlag(3)).unwrap().value, counts(4, 5));
    assert_eq!(act(c, &ids, CardAction::Suspend).unwrap().value, counts(3, 5));
    assert_eq!(change(c, &ids, DeckTarget::Deck(w.other)).unwrap().value, counts(4, 5));
    let before = dump(c);
    for action in [CardAction::Suspend, CardAction::SetFlag(1), CardAction::Unbury] {
        assert!(matches!(act(c, &[], action.clone()), Err(EngineError::InvalidArgument { .. })));
        assert!(matches!(
            act(c, &[ids[0], ids[1], ids[0]], action),
            Err(EngineError::InvalidArgument { .. })
        ));
    }
    assert_eq!(dump(c), before);
}

/// The scheduling actions refuse while a review session is active, as
/// `sched::ops::run` does; admitted by the review service they run, and the
/// session no longer shows a suspended card. Flag needs no admission.
#[test]
fn a_review_session_admits_the_scheduling_actions() {
    let h = session_support::Harness::new(3);
    let (id, head) = h.start();
    let target = head.items[0].card_id;
    let before = dump(&h.collection);
    assert_eq!(act(&h.collection, &[target], CardAction::Suspend).unwrap_err(), EngineError::SessionActive);
    assert_eq!(dump(&h.collection), before);
    let cancel = CancelToken::new();
    let done = h
        .review
        .admit_card_action(
            &cancel,
            |c| {
                cards::run_admitted(
                    c,
                    &CardTarget::Cards(vec![target]),
                    &CardAction::Suspend,
                    Arc::clone(&cancel),
                    ProgressSink::new(),
                )
            },
            |done| done.seq,
        )
        .unwrap();
    assert_eq!(done.value, counts(1, 1));
    assert_eq!(read(&h.collection, target).queue, CardQueue::Suspended);
    assert!(!h.review.head(id, 3).unwrap().items.iter().any(|item| item.card_id == target));
    assert_eq!(act(&h.collection, &[target], CardAction::SetFlag(4)).unwrap().value, counts(1, 1));
    h.review.end(id).unwrap();
}

/// B1, H12: a notes-mode selection of a search handle acts on every card of
/// each selected note, also a card the search did not match.
#[test]
fn b1_a_notes_mode_selection_acts_on_every_card_of_its_notes() {
    struct Events(Mutex<Vec<SearchEvent>>);
    impl SearchListener for Events {
        fn event(&self, event: SearchEvent) {
            self.0.lock().unwrap().push(event);
        }
    }
    let w = world();
    let c = &w.h.collection;
    let (first, second, lone) = c
        .run(OpSpec::named("two cards", OpSource::Maintenance), |ctx| {
            let s = ctx.scope();
            let nid = s.add_note(&common::note(&["pair", "back"]))?;
            let first = s.add_card(&common::card(nid, 0))?;
            let mut other = common::card(nid, 1);
            other.queue = CardQueue::Suspended;
            let second = s.add_card(&other)?;
            let lone = add_card(s, "lone", DeckId(1), |_| {});
            Ok((first, second, lone))
        })
        .unwrap()
        .value;
    let events = Arc::new(Events(Mutex::new(Vec::new())));
    let session = w.h.engine.open_search_session(events.clone());
    // `is:new` leaves the suspended card out; notes mode selects its note.
    session.start_search("is:new -is:suspended", SearchMode::Notes, SortSpec::new("noteFld", SortDirection::Ascending), None);
    let mut waits = 6_000;
    let (handle, revision) = loop {
        let ended = events.0.lock().unwrap().iter().find_map(|event| match event {
            SearchEvent::Complete { handle, revision, .. } => Some(Ok((*handle, *revision))),
            SearchEvent::Failed { .. } | SearchEvent::Cancelled { .. } => Some(Err(format!("{event:?}"))),
            _ => None,
        });
        match ended {
            Some(Ok(found)) => break found,
            Some(Err(event)) => panic!("the search ended without a result: {event}"),
            None => {
                waits -= 1;
                assert!(waits > 0, "the search did not end");
            }
        }
        std::thread::sleep(Duration::from_millis(5));
    };
    // Rows in noteFld order: "lone", then "pair".
    let rows = std::iter::once(1..2).collect();
    let selection = session.selection(handle, revision, rows);
    let done = cards::run(
        c,
        &CardTarget::Selection(selection),
        &CardAction::SetFlag(6),
        CancelToken::new(),
        ProgressSink::new(),
    )
    .unwrap();
    assert_eq!(done.value, counts(2, 2));
    assert_eq!((read(c, first).flags, read(c, second).flags, read(c, lone).flags), (6, 6, 0));
}

// --- V8.1: the note actions ------------------------------------------------------

fn tags_of(c: &Collection, id: NoteId) -> String {
    c.reader(ReaderRole::Query).unwrap().note(id).unwrap().unwrap().tags
}

/// B9: a TAB, LF or VT in the entry separates two tags for Add Tags and for
/// Remove Tags; a text holding no tag is refused with nothing written.
#[test]
fn b9_add_and_remove_tags_split_at_whitespace_runs() {
    let w = world();
    let c = &w.h.collection;
    let notes: Vec<NoteId> = c
        .run(OpSpec::named("notes", OpSource::Maintenance), |ctx| {
            (0..2)
                .map(|i| Ok(ctx.scope().add_note(&common::note(&[&format!("t{i}"), "back"]))?))
                .collect()
        })
        .unwrap()
        .value;
    let before = dump(c);
    let done = note_act(c, &notes, NoteAction::AddTags("alpha\tbeta \n\u{0b}gamma".into())).unwrap();
    assert_eq!(done.value.counts, counts(2, 2));
    assert_eq!(done.value.cards_removed, None);
    assert_eq!(tags_of(c, notes[0]), " alpha beta gamma ");
    exact_undo_redo(c, &before, "Update Tag");
    let added = dump(c);
    let done = note_act(c, &notes[..1], NoteAction::RemoveTags("alpha\tgamma".into())).unwrap();
    assert_eq!(done.value.counts, counts(1, 1));
    assert_eq!(tags_of(c, notes[0]), " beta ");
    exact_undo_redo(c, &added, "Remove Tag");
    let now = dump(c);
    let undo = undo_name(c);
    for text in ["", " \t\n"] {
        for action in [NoteAction::AddTags(text.into()), NoteAction::RemoveTags(text.into())] {
            assert!(matches!(note_act(c, &notes, action), Err(EngineError::InvalidArgument { .. })));
        }
    }
    assert!(matches!(
        note_actions::run(
            c,
            &NoteTarget::AllNotes,
            &NoteAction::AddTags("x".into()),
            CancelToken::new(),
            ProgressSink::new()
        ),
        Err(EngineError::InvalidArgument { .. })
    ));
    assert_eq!(dump(c), now);
    assert_eq!(undo_name(c), undo);
}

/// B11, K5: Delete Notes removes every card of each note in one "Delete
/// Note" step and reports the notes and the cards removed; a deleted id
/// counts among those considered. Run as a job, the outcome carries both
/// counts. Undo restores every row.
#[test]
fn b11_delete_notes_reports_notes_and_cards() {
    let w = world();
    let c = &w.h.collection;
    let notes: Vec<NoteId> = c
        .run(OpSpec::named("notes", OpSource::Maintenance), |ctx| {
            let s = ctx.scope();
            let two = s.add_note(&common::note(&["two", "back"]))?;
            s.add_card(&common::card(two, 0))?;
            s.add_card(&common::card(two, 1))?;
            let one = s.add_note(&common::note(&["one", "back"]))?;
            s.add_card(&common::card(one, 0))?;
            let gone = s.add_note(&common::note(&["gone", "back"]))?;
            s.remove_note_and_cards(gone)?;
            // A damaged card whose note row is missing: not removed, not
            // counted.
            s.add_card(&common::card(NoteId(424_242), 0))?;
            Ok(vec![two, one, gone, NoteId(424_242)])
        })
        .unwrap()
        .value;
    c.clear_undo(meguri_engine::ClearCause::CheckDatabase).unwrap();
    let before = dump(c);
    let job = note_actions::start(&w.h.engine, c.clone(), NoteTarget::Notes(notes.clone()), NoteAction::DeleteNotes).unwrap();
    let outcome = job.wait(Duration::from_secs(30)).expect("a result").unwrap();
    assert_eq!(outcome.counts, counts(2, 4));
    assert_eq!(outcome.cards_removed, Some(3));
    assert_eq!(outcome.undo.map(|u| u.name), Some("Delete Note"));
    let r = c.reader(ReaderRole::Query).unwrap();
    assert!(notes.iter().all(|id| r.note(*id).unwrap().is_none()));
    assert_eq!(r.cards_of_note(NoteId(424_242)).unwrap().len(), 1, "the damaged card stays");
    drop(r);
    exact_undo_redo(c, &before, "Delete Note");
}

/// K2: the Change Deck and card-action jobs report their counts and steps.
#[test]
fn the_jobs_report_their_counts_and_steps() {
    let w = world();
    let c = &w.h.collection;
    let ids = queues(&w);
    let job = change_deck::start(
        &w.h.engine,
        c.clone(),
        CardTarget::Cards(ids.clone()),
        DeckTarget::Deck(w.other),
    )
    .unwrap();
    let outcome = job.wait(Duration::from_secs(30)).expect("a result").unwrap();
    assert_eq!(outcome.counts, counts(4, 4));
    assert_eq!(outcome.undo.map(|u| u.name), Some("Change Deck"));
    assert_eq!(outcome.cards_removed, None);
    assert!(outcome.changes.cards.iter().any(|change| change.id == ids[0]));
    let job = cards::start(&w.h.engine, c.clone(), CardTarget::Cards(ids.clone()), CardAction::Unbury).unwrap();
    let outcome = job.wait(Duration::from_secs(30)).expect("a result").unwrap();
    assert_eq!(outcome.counts, counts(2, 4));
    assert_eq!(outcome.undo.map(|u| u.name), Some("Unbury/Unsuspend"));
    let refused = cards::start(&w.h.engine, c.clone(), CardTarget::Cards(vec![]), CardAction::Bury).unwrap();
    assert!(matches!(
        refused.wait(Duration::from_secs(30)).expect("a result"),
        Err(EngineError::InvalidArgument { .. })
    ));
}
