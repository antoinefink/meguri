//! E2 integration: real U41 gather, U44 answers and registered U25 restoration.
mod common;
use meguri_engine::{
    OpSource, OpSpec, UndoScope,
    sched::{answer, queue::QueueProvider},
    session::held::HeldQueue,
};
use meguri_model::{
    clock::Clock,
    ids::{CardId, DeckId},
};
use meguri_sched::{answer::Rating, queue::Kind, states::fsrs_glue::FsrsCache};
use meguri_storage::{ConfigValue, ReadRows, ReaderRole};

fn setup() -> (common::Harness, Vec<CardId>) {
    let h = common::fresh();
    let ids = h
        .collection
        .run(OpSpec::named("held fixture", OpSource::UserNonUndoable), |ctx| {
            let mut ids = Vec::new();
            for i in 0..4 {
                let nid = ctx.scope().add_note(&common::note(&[&format!("front{i}"), "back"]))?;
                let mut card = common::card(nid, 0);
                card.due = i + 1;
                ids.push(ctx.scope().add_card(&card)?);
            }
            Ok(ids)
        })
        .unwrap()
        .value;
    (h, ids)
}
fn request(h: &common::Harness, held: &HeldQueue, id: CardId, rating: Rating) -> answer::AnswerRequest {
    answer::predict(&h.collection, id, &mut FsrsCache::default(), Some(held.cache()))
        .unwrap()
        .answer(rating, 1300)
}
fn head(held: &HeldQueue, now: i64) -> Vec<CardId> {
    held.read(now).0.iter().map(|e| e.id).collect()
}

#[test]
fn fsrs_off_compatibility_queue_reads_write_nothing_and_never_select() {
    let (h, _) = setup();
    let c = &h.collection;
    c.run(OpSpec::named("fixture selection", OpSource::Maintenance), |ctx| {
        ctx.scope().set_config("fsrs", &ConfigValue::Bool(false))?;
        ctx.scope().set_config("curDeck", &ConfigValue::Integer(999))?;
        Ok(())
    })
    .unwrap();
    let before = common::steps::dump(c);
    let mod_before = c.reader(ReaderRole::Query).unwrap().col().unwrap().modified;
    let held = HeldQueue::build(c, Some(DeckId(1))).unwrap();
    assert_eq!(held.read(h.clock.now_ms().div_euclid(1000)).1.new, 4);
    assert_eq!(common::steps::dump(c), before);
    assert_eq!(c.reader(ReaderRole::Query).unwrap().col().unwrap().modified, mod_before);
    assert_eq!(
        c.reader(ReaderRole::Query).unwrap().config_value("curDeck").unwrap(),
        Some(ConfigValue::Integer(999))
    );
}

#[test]
fn held_queue_progresses_while_unheld_u41_still_gathers_fresh() {
    let (h, ids) = setup();
    let held = HeldQueue::build(&h.collection, Some(DeckId(1))).unwrap();
    let now = h.clock.now_ms().div_euclid(1000);
    assert_eq!(head(&held, now), ids);
    let result = held.answer(request(&h, &held, ids[0], Rating::Again)).unwrap();
    assert_eq!(result.value.metadata.from_queue, Some(Kind::New));
    assert_eq!(head(&held, now)[0], ids[1]);
    assert_eq!(held.read(now).1.new, 3);
    let fresh = QueueProvider::default().queue(&h.collection, Some(DeckId(1))).unwrap();
    assert_eq!(fresh.value.main.len(), 3);
    assert_eq!(fresh.value.learning.len(), 1);
}

#[test]
fn not_at_head_and_stale_answers_preserve_collection_and_presentation() {
    let (h, ids) = setup();
    let held = HeldQueue::build(&h.collection, Some(DeckId(1))).unwrap();
    let before = common::steps::dump(&h.collection);
    assert!(held.answer(request(&h, &held, ids[2], Rating::Good)).is_err());
    assert_eq!(common::steps::dump(&h.collection), before);
    let mut stale = request(&h, &held, ids[0], Rating::Good);
    stale.state_token = [0; 32];
    assert!(matches!(
        held.answer(stale),
        Err(meguri_engine::EngineError::Stale { .. })
    ));
    assert_eq!(common::steps::dump(&h.collection), before);
    assert_eq!(head(&held, h.clock.now_ms().div_euclid(1000)), ids);
}

#[test]
fn global_answer_undo_redo_uses_registered_queue_restorer() {
    let (h, ids) = setup();
    let held = HeldQueue::build(&h.collection, Some(DeckId(1))).unwrap();
    let now = h.clock.now_ms().div_euclid(1000);
    held.answer(request(&h, &held, ids[0], Rating::Again)).unwrap();
    let after = held.read(now);
    h.collection.undo(UndoScope::Global).unwrap();
    assert_eq!(head(&held, now), ids);
    h.collection.redo(UndoScope::Global).unwrap();
    assert_eq!(held.read(now), after);
}

#[test]
fn actual_redo_across_queue_invalidation_rebuilds_from_committed_rows() {
    let (h, ids) = setup();
    let mut held = HeldQueue::build(&h.collection, Some(DeckId(1))).unwrap();
    let now = h.clock.now_ms().div_euclid(1000);
    held.answer(request(&h, &held, ids[0], Rating::Easy)).unwrap();
    h.collection.undo(UndoScope::Global).unwrap();
    held.invalidate();
    held.refresh().unwrap();
    assert_eq!(head(&held, now), ids);
    h.collection.redo(UndoScope::Global).unwrap();
    assert!(!held.is_current(), "old P9 build record requests a fresh build");
    held.refresh().unwrap();
    assert!(!head(&held, now).contains(&ids[0]));
    assert_eq!(head(&held, now), ids[1..]);
}
