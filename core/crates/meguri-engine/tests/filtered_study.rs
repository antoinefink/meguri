//! Synthetic F1/F2/F5 regression coverage through production operations.
mod common;
use common::{Harness, card, note, revlog, NOW_MS};
use meguri_engine::{OpSource, OpSpec, UndoScope, decks::{self, filtered::{self, Definition, Term}, custom_study::{self, Mode, State, Request}}};
use meguri_model::{card::{Card, CardQueue, CardType}, card_data::CardData, ids::{CardId, DeckId, NoteId}, proto::{ProtoBlob, deck::{DeckKind, DeckKindMessage}}};
use meguri_storage::{ReadRows, ReaderRole, Record, ConfigValue};

fn definition(name: &str, search: &str, limit: u32, order: u32, reschedule: bool) -> Definition {
    Definition { name: name.into(), terms: vec![Term { search: search.into(), limit, order }], reschedule, ..Definition::default() }
}
fn source(h: &Harness, name: &str) -> DeckId { decks::add(&h.collection, name.into()).unwrap().value.id }
fn seed(h: &Harness, deck: DeckId, cards: Vec<Card>) -> Vec<CardId> {
    h.collection.run(OpSpec::named("seed_filtered", OpSource::UserNonUndoable), move |ctx| {
        let mut ids = Vec::new();
        for mut c in cards {
            c.note_id = ctx.scope().add_note(&note(&["front", "back"]))?;
            c.deck_id = deck;
            ids.push(ctx.scope().add_card(&c)?);
        }
        Ok(ids)
    }).unwrap().value
}
fn new_card() -> Card { card(NoteId(0), 0) }
fn row(h: &Harness, id: CardId) -> Card {
    h.collection.reader(ReaderRole::Query).unwrap().card(id).unwrap().unwrap()
}
fn members(h: &Harness, id: DeckId) -> Vec<Card> {
    let mut cards = h.collection.reader(ReaderRole::Query).unwrap().cards_of_deck(id).unwrap();
    cards.sort_by_key(|c| c.due);
    cards
}
fn request(mode: Mode, amount: i32) -> Request {
    Request { mode, amount, state: State::All, include_tags: vec![], exclude_tags: vec![] }
}
fn image(h: &Harness, id: CardId) -> Vec<meguri_storage::Cell> { row(h, id).to_cells() }

#[test]
fn overlapping_terms_successive_rebuilds_and_live_edits() {
    let h = common::fresh();
    let home = source(&h, "Home");
    let other = source(&h, "Other");
    let ids = seed(&h, home, vec![new_card(); 6]);
    let mut suspended = new_card(); suspended.queue = CardQueue::Suspended;
    let blocked = seed(&h, other, vec![suspended])[0];
    let mut learning = new_card(); learning.card_type = CardType::Learning; learning.queue = CardQueue::Learning;
    let learner = seed(&h, home, vec![learning])[0];
    let held = filtered::save(&h.collection, None, definition("Held", &format!("cid:{}", ids[5].0), 1, 5, true), false).unwrap().value;
    let mut def = definition("Study", "deck:Home OR deck:Other", 2, 5, true);
    def.terms.push(Term { search: "deck:Home OR deck:Other".into(), limit: 2, order: 5 });
    let built = filtered::save(&h.collection, None, def, false).unwrap();
    let target = built.value;
    assert_eq!(built.undo.unwrap().name, "Build Deck");
    for _ in 0..2 {
        assert_eq!(members(&h, target).iter().map(|c| c.id).collect::<Vec<_>>(), ids[..4]);
        filtered::rebuild(&h.collection, target).unwrap();
    }
    assert_eq!(row(&h, blocked).queue, CardQueue::Suspended);
    assert_eq!(row(&h, ids[5]).deck_id, held);
    h.collection.run(OpSpec::named("live_edit", OpSource::UserNonUndoable), move |ctx| {
        let mut c = ctx.scope().card(ids[0])?.unwrap(); c.queue = CardQueue::BuriedByUser; ctx.scope().put(&c)?;
        Ok(())
    }).unwrap();
    filtered::rebuild(&h.collection, target).unwrap();
    assert_eq!(members(&h, target).len(), 4);
    assert_eq!(row(&h, learner).deck_id, home);
    // A standalone learning-card build ignores normal daily limits.
    let learning_deck = filtered::save(&h.collection, None, definition("Learning", &format!("cid:{}", learner.0), 1, 5, true), false).unwrap().value;
    assert_eq!(row(&h, learner).deck_id, learning_deck);
    assert_eq!(row(&h, learner).queue, CardQueue::Learning);
}

#[test]
fn failed_edits_are_atomic_and_empty_has_real_undo() {
    let h = common::fresh(); let home = source(&h, "Home");
    let ids = seed(&h, home, vec![new_card(); 3]);
    let target = filtered::save(&h.collection, None, definition("Study", "deck:Home", 2, 5, true), false).unwrap().value;
    let before = ids.iter().map(|id| image(&h, *id)).collect::<Vec<_>>();
    let saved = filtered::definition(&*h.collection.reader(ReaderRole::Query).unwrap(), target).unwrap();
    let mut bad = saved.clone(); bad.name = "Edited".into(); bad.terms.push(Term { search: "(".into(), limit: 2, order: 6 });
    assert!(filtered::save(&h.collection, Some(target), bad, false).is_err());
    let mut empty = saved.clone(); empty.terms[0].search = "cid:1".into();
    assert!(matches!(filtered::save(&h.collection, Some(target), empty.clone(), false),
        Err(meguri_engine::EngineError::DeckRefused(decks::Refusal::NoMatches))));
    assert_eq!(filtered::definition(&*h.collection.reader(ReaderRole::Query).unwrap(), target).unwrap(), saved);
    assert_eq!(ids.iter().map(|id| image(&h, *id)).collect::<Vec<_>>(), before);
    filtered::empty(&h.collection, target).unwrap();
    assert!(members(&h, target).is_empty());
    assert_eq!(row(&h, ids[0]).deck_id, home);
    h.collection.undo(UndoScope::Global).unwrap();
    assert_eq!(ids.iter().map(|id| image(&h, *id)).collect::<Vec<_>>(), before);
    filtered::save(&h.collection, Some(target), empty, true).unwrap();
    assert!(members(&h, target).is_empty());
    h.collection.undo(UndoScope::Global).unwrap();
    assert_eq!(ids.iter().map(|id| image(&h, *id)).collect::<Vec<_>>(), before);
    let normal = source(&h, "Conflict");
    assert!(filtered::save(&h.collection, None, definition("Conflict", "", 1, 1, true), true).is_err());
    assert!(!decks::is_filtered(&h.collection.reader(ReaderRole::Query).unwrap().deck(normal).unwrap().unwrap()));
    assert!(filtered::save(&h.collection, None, definition("Study::Child", "", 1, 1, true), true).is_err());
    for invalid in [11, u32::MAX] {
        assert!(filtered::save(&h.collection, Some(target), definition("Study", "", 1, invalid, true), false).is_err());
    }
}

#[test]
fn every_order_gathers_before_limiting_including_fsrs_and_history() {
    for (order, expected) in [(0, vec![0,1,2]), (2, vec![0,2,1]), (3, vec![1,2,0]), (4, vec![1,2,0]),
        (5, vec![0,1,2]), (6, vec![1,2,0]), (7, vec![2,1,0]), (8, vec![0,1,2]), (9, vec![2,1,0]), (10, vec![1,2,0])] {
        let h = common::fresh(); let home = source(&h, "Home");
        h.collection.run(OpSpec::named("collection_day", OpSource::UserNonUndoable), |ctx| {
            let mut col = ctx.scope().col()?; col.created.0 -= 100 * 86400; ctx.scope().put(&col)?; Ok(())
        }).unwrap();
        let today = h.collection.today().unwrap(); assert_eq!(today, 100);
        let mut cards = Vec::new();
        for i in 0..3 {
            let mut c = new_card(); c.card_type = CardType::Review; c.queue = CardQueue::Review;
            c.due = [100,70,90][i]; c.interval = [5,20,10][i]; c.lapses = [1,3,2][i];
            if matches!(order, 8 | 9) && i > 0 {
                c.data = CardData::parse(&format!("{{\"s\":{},\"d\":5,\"lrt\":{}}}",
                    if i == 1 {1} else {100}, NOW_MS/1000 - if i == 1 {30*86400} else {86400}));
            }
            cards.push(c);
        }
        let ids = seed(&h, home, cards);
        let log_ids = ids.clone();
        h.collection.run(OpSpec::named("history", OpSource::UserNonUndoable), move |ctx| {
            for &id in &log_ids[1..3] {
                let log = revlog(id);
                ctx.scope().add_revlog(&log)?;
            }
            Ok(())
        }).unwrap();
        let selected = filtered::save(&h.collection, None, definition("Study", "deck:Home", 2, order, true), false).unwrap().value;
        assert_eq!(members(&h, selected).iter().map(|c| c.id).collect::<Vec<_>>(), expected[..2].iter().map(|i| ids[*i]).collect::<Vec<_>>(), "order {order}");
    }
    let h = common::fresh(); let home = source(&h, "Home");
    let ids = seed(&h, home, vec![new_card(); 16]);
    let target = filtered::save(&h.collection, None, definition("Random", "deck:Home", 16, 1, true), false).unwrap().value;
    let first = members(&h, target).iter().map(|c| c.id).collect::<Vec<_>>();
    filtered::rebuild(&h.collection, target).unwrap();
    let next = members(&h, target).iter().map(|c| c.id).collect::<Vec<_>>();
    assert_ne!(first, next);
    assert_eq!(first.into_iter().collect::<std::collections::BTreeSet<_>>(), ids.into_iter().collect());
}

#[test]
fn edit_imported_messages_keeps_unknown_terms_and_legacy_delay() {
    let h = common::fresh(); let home = source(&h, "Home"); seed(&h, home, vec![new_card(); 4]);
    let target = source(&h, "Imported");
    h.collection.run(OpSpec::named("imported_definition", OpSource::UserNonUndoable), move |ctx| {
        // Independent wire fixture: field 99 at outer, filtered and term
        // levels; both terms have their own unknown value.
        let payload = vec![8,1,18,7,16,2,24,5,152,6,2,18,7,16,2,24,5,152,6,3,32,7,152,6,4];
        let mut wire = vec![18, payload.len() as u8]; wire.extend(payload); wire.extend([152,6,1]);
        let mut d = ctx.scope().deck(target)?.unwrap(); d.kind = ProtoBlob::<DeckKindMessage>::decode(&wire).unwrap();
        ctx.scope().put(&d)?; Ok(())
    }).unwrap();
    let mut def = filtered::definition(&*h.collection.reader(ReaderRole::Query).unwrap(), target).unwrap();
    def.terms[0].search = "deck:Home".into(); def.terms[1].search = "deck:Home".into(); def.reschedule = false;
    filtered::save(&h.collection, Some(target), def.clone(), false).unwrap();
    h.engine.close().unwrap();
    let reopened = h.engine.open_raw(&h.path, common::open_config(&h.clock)).unwrap();
    let reader = reopened.reader(ReaderRole::Query).unwrap();
    let d = reader.deck(target).unwrap().unwrap();
    assert_eq!(d.kind.unknown_fields().iter().next().unwrap().bytes(), [152,6,1]);
    let DeckKind::Filtered(f) = &d.kind.kind else { panic!() };
    assert_eq!(f.preview_delay, 7);
    assert_eq!(f.unknown_fields().iter().next().unwrap().bytes(), [152,6,4]);
    assert_eq!(f.search_terms[0].unknown_fields().iter().next().unwrap().bytes(), [152,6,2]);
    assert_eq!(f.search_terms[1].unknown_fields().iter().next().unwrap().bytes(), [152,6,3]);
    assert_eq!(filtered::definition(&*reader, target).unwrap(), def);
}

#[test]
fn preview_return_restores_zero_and_nonzero_days_for_empty_rebuild_and_delete() {
    for action in 0..3 {
        let mut h = common::fresh(); let home = source(&h, "Home");
        let mut zero = new_card(); zero.card_type = CardType::Review; zero.queue = CardQueue::Review; zero.due = 0; zero.interval = 21;
        let mut nonzero = zero.clone(); nonzero.due = 103;
        let ids = seed(&h, home, vec![zero, nonzero]);
        let target = filtered::save(&h.collection, None, definition("Preview", "deck:Home", 2, 5, false), false).unwrap().value;
        assert_eq!(row(&h, ids[0]).due, 0);
        let changed = ids.clone();
        h.collection.run(OpSpec::named("preview_repeat_fixture", OpSource::UserNonUndoable), move |ctx| {
            for id in changed.iter() { let mut c = ctx.scope().card(*id)?.unwrap(); c.queue = CardQueue::Preview; c.due = (NOW_MS/1000 +60) as i32; ctx.scope().put(&c)?; }
            // Make Rebuild release without gathering again.
            if action == 1 { let mut d = ctx.scope().deck(target)?.unwrap(); let DeckKind::Filtered(ref mut f) = d.kind.kind else { panic!() }; f.search_terms[0].search = "cid:1".into(); ctx.scope().put(&d)?; }
            Ok(())
        }).unwrap();
        h.engine.close().unwrap();
        h.collection = h.engine.open_raw(&h.path, common::open_config(&h.clock)).unwrap();
        let before = ids.iter().map(|id| image(&h, *id)).collect::<Vec<_>>();
        match action {
            0 => { filtered::empty(&h.collection, target).unwrap(); },
            1 => { filtered::rebuild(&h.collection, target).unwrap(); },
            _ => {},
        }
        if action == 2 {
            h.collection.run(OpSpec::named("delete_preview", OpSource::UserUndoable).undo_step("Delete Deck", meguri_engine::StepKind::Other), move |ctx| {
                decks::delete_in(ctx.scope(), &[target])?; Ok(())
            }).unwrap();
        }
        for (id, due) in ids.iter().zip([0,103]) { let c = row(&h, *id); assert_eq!((c.deck_id,c.due,c.interval,c.queue), (home,due,21,CardQueue::Review)); }
        h.collection.undo(UndoScope::Global).unwrap();
        assert_eq!(ids.iter().map(|id| image(&h, *id)).collect::<Vec<_>>(), before);
    }
}

#[test]
fn paged_members_all_return_home_after_reopen_and_one_undo_restores_them() {
    for rebuild in [false, true] {
        let mut h = common::fresh(); let home = source(&h, "Paged home");
        // More than two 250-id pages, including a partial final page.
        let cards = (0..513).map(|i| {
            let mut c = new_card(); c.card_type = CardType::Review; c.queue = CardQueue::Review;
            c.due = if i % 2 == 0 { 0 } else { 103 }; c.interval = 21;
            c
        }).collect();
        let ids = seed(&h, home, cards);
        let target = filtered::save(&h.collection, None,
            definition("Paged preview", &format!("did:{}", home.0), 513, 5, false), false).unwrap().value;
        let repeats = ids.clone();
        h.collection.run(OpSpec::named("paged_preview_repeat_fixture", OpSource::UserNonUndoable), move |ctx| {
            for id in repeats {
                let mut c = ctx.scope().card(id)?.unwrap();
                c.queue = CardQueue::Preview; c.due = (NOW_MS / 1000 + 60) as i32;
                ctx.scope().put(&c)?;
            }
            if rebuild {
                let mut deck = ctx.scope().deck(target)?.unwrap();
                let DeckKind::Filtered(ref mut f) = deck.kind.kind else { panic!() };
                f.search_terms[0].search = "cid:1".into();
                ctx.scope().put(&deck)?;
            }
            Ok(())
        }).unwrap();
        h.engine.close().unwrap();
        h.collection = h.engine.open_raw(&h.path, common::open_config(&h.clock)).unwrap();
        assert_eq!(members(&h, target).len(), 513);
        let before = ids.iter().map(|id| image(&h, *id)).collect::<Vec<_>>();
        if rebuild { filtered::rebuild(&h.collection, target).unwrap(); }
        else { filtered::empty(&h.collection, target).unwrap(); }
        assert!(members(&h, target).is_empty());
        // Inspect persisted rows, including members beyond the first page.
        for (i, id) in ids.iter().enumerate() {
            let c = row(&h, *id);
            assert_eq!((c.deck_id, c.original_deck_id, c.original_due, c.queue, c.due, c.interval),
                (home, DeckId(0), 0, CardQueue::Review, if i % 2 == 0 { 0 } else { 103 }, 21));
        }
        h.collection.undo(UndoScope::Global).unwrap();
        assert_eq!(ids.iter().map(|id| image(&h, *id)).collect::<Vec<_>>(), before);
    }
}

#[test]
fn custom_tag_literals_hierarchy_remembered_choices_rollback_and_undo() {
    let h = common::fresh();
    let home = source(&h, "Scope\"*::OR"); let child = source(&h, "Scope\"*::OR::Child"); let other = source(&h, "Other");
    let a = seed(&h, home, vec![new_card(); 2]);
    let b = seed(&h, child, vec![new_card()])[0];
    let outsider = seed(&h, other, vec![new_card()])[0];
    let selected_ids = [a[0], a[1], b, outsider];
    h.collection.run(OpSpec::named("tag_fixture", OpSource::UserNonUndoable), move |ctx| {
        for (i, id) in selected_ids.iter().enumerate() {
            let c = ctx.scope().card(*id)?.unwrap();
            let mut n = ctx.scope().note(c.note_id)?.unwrap();
            n.tags = if i == 1 { " topic\"*_(or)::child skip:* ".into() } else { " topic\"*_(or)::child ".into() };
            ctx.scope().put(&n)?;
        }
        Ok(())
    }).unwrap();
    let mut r = request(Mode::ByState, 100); r.state = State::New;
    r.include_tags = vec!["topic\"*_(or)".into(), "absent".into()]; r.exclude_tags = vec!["skip:*".into()];
    let selected = custom_study::custom(&h.collection, home, r.clone()).unwrap();
    assert_eq!(selected.undo.unwrap().name, "Custom Study");
    assert_eq!(members(&h, selected.value).iter().map(|c| c.id).collect::<Vec<_>>(), [a[0], b]);
    assert_eq!(row(&h, outsider).deck_id, other);
    assert_eq!(row(&h, a[1]).deck_id, home);
    let defaults = custom_study::defaults(&h.collection, home).unwrap();
    assert_eq!((defaults.include_tags, defaults.exclude_tags), (r.include_tags.clone(), r.exclude_tags.clone()));
    assert_eq!(decks::current(&*h.collection.reader(ReaderRole::Query).unwrap()).unwrap(), selected.value);
    let before = members(&h, selected.value).iter().map(Record::to_cells).collect::<Vec<_>>();
    r.include_tags = vec!["unmatched".into()];
    assert!(custom_study::custom(&h.collection, home, r).is_err());
    assert_eq!(members(&h, selected.value).iter().map(Record::to_cells).collect::<Vec<_>>(), before);
    assert_eq!(custom_study::defaults(&h.collection, home).unwrap().include_tags, ["topic\"*_(or)", "absent"]);
    h.collection.undo(UndoScope::Global).unwrap();
    assert!(h.collection.reader(ReaderRole::Query).unwrap().deck(selected.value).unwrap().is_none());
    assert_eq!(row(&h, a[0]).deck_id, home);
    assert!(custom_study::defaults(&h.collection, home).unwrap().include_tags.is_empty());
}

#[test]
fn all_custom_search_modes_and_states_reuse_the_session() {
    let h = common::fresh(); let home = source(&h, "Home");
    let new = seed(&h, home, vec![new_card()])[0];
    let mut due = new_card(); due.card_type = CardType::Review; due.queue = CardQueue::Review; due.due = 0;
    let due_id = seed(&h, home, vec![due.clone()])[0];
    due.due = 5; let ahead = seed(&h, home, vec![due])[0];
    h.collection.run(OpSpec::named("forgotten_fixture", OpSource::UserNonUndoable), move |ctx| {
        let mut log = revlog(due_id); log.button = 1; ctx.scope().add_revlog(&log)?; Ok(())
    }).unwrap();
    let mut previous = None;
    for (r, expected, reschedule, order) in [
        (request(Mode::Forgotten, 1), vec![due_id], false, 1),
        (request(Mode::Ahead, 1), vec![due_id], true, 6),
        (request(Mode::PreviewNew, 1), vec![new], false, 5),
        (Request { state: State::New, ..request(Mode::ByState, 100) }, vec![new], true, 5),
        (Request { state: State::Due, ..request(Mode::ByState, 100) }, vec![due_id], true, 6),
        (Request { state: State::Review, ..request(Mode::ByState, 100) }, vec![due_id, ahead], true, 1),
        (request(Mode::ByState, 100), vec![new, due_id, ahead], false, 1),
    ] {
        let limit = if r.mode == Mode::ByState { r.amount as u32 } else { 99_999 };
        let target = custom_study::custom(&h.collection, home, r).unwrap().value;
        if let Some(previous) = previous { assert_eq!(target, previous); }
        previous = Some(target);
        let ids = members(&h, target).iter().map(|c| c.id).collect::<std::collections::BTreeSet<_>>();
        assert_eq!(ids, expected.into_iter().collect());
        let def = filtered::definition(&*h.collection.reader(ReaderRole::Query).unwrap(), target).unwrap();
        assert_eq!((def.reschedule, def.terms[0].order, def.terms[0].limit), (reschedule, order, limit));
        filtered::empty(&h.collection, target).unwrap();
    }
    let limited = custom_study::custom(&h.collection, home, request(Mode::ByState, 1)).unwrap().value;
    assert_eq!(members(&h, limited).len(), 1);
    filtered::empty(&h.collection, limited).unwrap();
    assert!(custom_study::custom(&h.collection, home, request(Mode::Forgotten, 31)).is_err());
    assert!(custom_study::custom(&h.collection, home, request(Mode::Ahead, 0)).is_err());
    assert!(custom_study::custom(&h.collection, home, request(Mode::PreviewNew, -1)).is_err());
    assert!(custom_study::custom(&h.collection, home, request(Mode::ByState, 0)).is_err());
    let target = previous.unwrap();
    assert!(custom_study::custom(&h.collection, target, request(Mode::ByState, 100)).is_err());
}

#[test]
fn daily_extensions_reset_stale_counters_respect_parents_and_expire() {
    let h = common::fresh(); let parent = source(&h, "Parent"); let home = source(&h, "Parent::Home");
    let child = source(&h, "Parent::Home::Child");
    seed(&h, home, vec![new_card(); 2]); seed(&h, child, vec![new_card(); 3]);
    let mut review = new_card(); review.card_type = CardType::Review; review.queue = CardQueue::Review; review.due = 0;
    seed(&h, child, vec![review]);
    h.collection.run(OpSpec::named("stale_counters", OpSource::UserNonUndoable), move |ctx| {
        for id in [parent,home,child] { let mut d = ctx.scope().deck(id)?.unwrap(); d.common.last_day_studied = 999;
            d.common.new_studied = 17; d.common.review_studied = 23; ctx.scope().put(&d)?; }
        Ok(())
    }).unwrap();
    let d = custom_study::defaults(&h.collection, home).unwrap(); assert_eq!((d.available_new,d.available_review),(5,1));
    custom_study::custom(&h.collection, home, request(Mode::IncreaseNew, 7)).unwrap();
    let count = |id| h.collection.reader(ReaderRole::Query).unwrap().deck(id).unwrap().unwrap().common.new_studied;
    assert_eq!((count(home),count(parent),count(child)),(-7,17,17));
    assert_eq!(custom_study::defaults(&h.collection, home).unwrap().extend_new, 7);
    h.collection.run(OpSpec::named("parent_limits", OpSource::UserNonUndoable), |ctx| { ctx.scope().set_config("applyAllParentLimits", &ConfigValue::Bool(true))?; Ok(()) }).unwrap();
    custom_study::custom(&h.collection, home, request(Mode::IncreaseReview, 4)).unwrap();
    let reader = h.collection.reader(ReaderRole::Query).unwrap();
    assert_eq!(reader.deck(home).unwrap().unwrap().common.review_studied, -4);
    assert_eq!(reader.deck(parent).unwrap().unwrap().common.review_studied, -4);
    assert_eq!(reader.deck(child).unwrap().unwrap().common.review_studied, 23);
    drop(reader);
    custom_study::custom(&h.collection, home, request(Mode::IncreaseNew, -2)).unwrap();
    assert_eq!(count(home),-5);
    assert_eq!(custom_study::defaults(&h.collection, home).unwrap().extend_new,7);
    h.collection.undo(UndoScope::Global).unwrap(); assert_eq!(count(home),-7);
    h.clock.set_now_ms(h.collection.next_day_at().unwrap()*1000+1000);
    custom_study::custom(&h.collection, home, request(Mode::IncreaseNew, 3)).unwrap();
    assert_eq!((count(home),count(parent),count(child)),(-3,-3,17));
    assert_eq!(custom_study::defaults(&h.collection, home).unwrap().extend_review,4);
}

#[test]
fn custom_normal_name_conflict_and_cancel_leave_rows_untouched() {
    let h = common::fresh(); let home = source(&h, "Home"); let collision = source(&h, "Custom Study Session");
    let id = seed(&h, home, vec![new_card()])[0]; let before = image(&h,id);
    assert!(custom_study::custom(&h.collection,home,request(Mode::ByState,100)).is_err());
    assert_eq!(image(&h,id),before);
    assert!(!decks::is_filtered(&h.collection.reader(ReaderRole::Query).unwrap().deck(collision).unwrap().unwrap()));
    let cancel = meguri_engine::CancelToken::new(); cancel.cancel();
    assert!(custom_study::custom_controlled(&h.collection,home,request(Mode::IncreaseNew,5),cancel).is_err());
    assert_eq!(custom_study::defaults(&h.collection,home).unwrap().extend_new,0);
}

#[test]
fn due_order_compares_day_and_intraday_times_and_keeps_nonpositive_dues() {
    let h = common::fresh(); let home = source(&h,"Home");
    let mut day = new_card(); day.card_type = CardType::Review; day.queue = CardQueue::Review; day.due = 1;
    let mut time = day.clone(); time.card_type = CardType::Learning; time.queue = CardQueue::Learning; time.due = (NOW_MS/1000+10) as i32;
    let mut future = day.clone(); future.due = 2;
    let ids = seed(&h,home,vec![future,time,day]);
    let target = filtered::save(&h.collection,None,definition("Due","deck:Home",3,6,true),false).unwrap().value;
    // The collection is on day zero: the intraday card comes before days 1/2.
    assert_eq!(members(&h,target).iter().map(|c| c.id).collect::<Vec<_>>(),[ids[1],ids[2],ids[0]]);
    filtered::empty(&h.collection,target).unwrap();
    let mut zero = new_card(); zero.due = 0;
    let mut negative = new_card(); negative.due = -4;
    let special = seed(&h,home,vec![zero,negative]);
    let target = filtered::save(&h.collection,Some(target),definition("Due","deck:Home",10,5,true),false).unwrap().value;
    assert_eq!(row(&h,special[0]).due,0); assert_eq!(row(&h,special[1]).due,-4);
    assert_eq!(row(&h,special[0]).deck_id,target);
}

#[test]
fn generated_scopes_keep_unicode_whitespace_decks_and_descendants_separate() {
    let h = common::fresh();
    let home = source(&h, "A\u{a0}B");
    let child = source(&h, "A\u{a0}B::Child");
    let lookalike = source(&h, "A B");
    let other_child = source(&h, "A B::Child");
    let mut review = new_card();
    review.card_type = CardType::Review; review.queue = CardQueue::Review; review.due = 0;
    let home_ids = [seed(&h, home, vec![new_card(), review.clone()]), seed(&h, child, vec![new_card(), review.clone()])].concat();
    let other_ids = [seed(&h, lookalike, vec![new_card(), review.clone()]), seed(&h, other_child, vec![new_card(), review])].concat();
    for (deck, expected, excluded) in [(home, &home_ids, &other_ids), (lookalike, &other_ids, &home_ids)] {
        let scope = decks::search_scope::deck(&*h.collection.reader(ReaderRole::Query).unwrap(), deck).unwrap();
        if deck == home { assert!(scope.starts_with("did:")); }
        // Both editor defaults use this exact operand, retaining descendants.
        for (state, indexes) in [("is:due", [1, 3]), ("is:new", [0, 2])] {
            let target = filtered::save(&h.collection, None,
                definition("Editor scope", &format!("{scope} {state}"), 100, 5, true), false).unwrap().value;
            let actual = members(&h, target).iter().map(|c| c.id).collect::<std::collections::BTreeSet<_>>();
            assert_eq!(actual, indexes.map(|i| expected[i]).into_iter().collect());
            filtered::empty(&h.collection, target).unwrap();
            // Reuse the editor deck without colliding with the next iteration.
            decks::rename(&h.collection, target, format!("Editor {} {state}", deck.0)).unwrap();
        }
        let target = custom_study::custom(&h.collection, deck, request(Mode::ByState, 100)).unwrap().value;
        let actual = members(&h, target).iter().map(|c| c.id).collect::<std::collections::BTreeSet<_>>();
        assert_eq!(actual, expected.iter().copied().collect());
        assert!(excluded.iter().all(|id| row(&h, *id).original_deck_id == DeckId(0)));
        filtered::empty(&h.collection, target).unwrap();
    }
}

#[test]
fn daily_adjustment_bounds_preserve_all_rows_on_rejection() {
    for mode in [Mode::IncreaseNew, Mode::IncreaseReview] {
        let h = common::fresh();
        let parent = source(&h, "Bounds"); let home = source(&h, "Bounds::Home");
        let child = source(&h, "Bounds::Home::Child");
        let ids = seed(&h, home, vec![new_card(); 2]);
        h.collection.run(OpSpec::named("parent limits", OpSource::UserNonUndoable), |ctx| {
            ctx.scope().set_config("applyAllParentLimits", &ConfigValue::Bool(true))?; Ok(())
        }).unwrap();
        let today = h.collection.today().unwrap();
        for delta in [-99_999, 0, 99_999] {
            let before = h.collection.reader(ReaderRole::Query).unwrap().deck(home).unwrap().unwrap();
            custom_study::custom(&h.collection, home, request(mode, delta)).unwrap();
            let r = h.collection.reader(ReaderRole::Query).unwrap();
            let after = r.deck(home).unwrap().unwrap();
            let counter = |d: &meguri_model::deck::Deck| if mode == Mode::IncreaseNew { d.common.new_studied } else { d.common.review_studied };
            let initial = if before.common.last_day_studied == today { counter(&before) } else { 0 };
            assert_eq!(counter(&after), initial - delta);
            assert_eq!(counter(&r.deck(parent).unwrap().unwrap()), counter(&after));
            assert_eq!(counter(&r.deck(child).unwrap().unwrap()), 0);
        }
        let deck_images = || h.collection.reader(ReaderRole::Query).unwrap().deck_rows().unwrap().iter().map(Record::to_cells).collect::<Vec<_>>();
        let before = deck_images();
        let cards = ids.iter().map(|id| image(&h, *id)).collect::<Vec<_>>();
        for delta in [-100_000, 100_000, i32::MIN, i32::MAX] {
            assert!(matches!(custom_study::custom(&h.collection, home, request(mode, delta)), Err(meguri_engine::EngineError::InvalidArgument { .. })));
            assert_eq!(deck_images(), before);
            assert_eq!(ids.iter().map(|id| image(&h, *id)).collect::<Vec<_>>(), cards);
        }
    }
}

#[test]
fn daily_new_adjustment_example_changes_production_queue_counts_across_rollover() {
    let h = common::fresh(); let home = source(&h, "Daily example");
    seed(&h, home, vec![new_card(); 32]);
    let today = h.collection.today().unwrap();
    h.collection.run(OpSpec::named("daily example", OpSource::UserNonUndoable), move |ctx| {
        let mut preset = ctx.scope().deck_config(meguri_model::ids::DeckConfigId(1))?.unwrap();
        preset.config.new_per_day = 10; preset.config.reviews_per_day = 200;
        ctx.scope().put(&preset)?;
        let mut deck = ctx.scope().deck(home)?.unwrap();
        deck.common.last_day_studied = today; deck.common.new_studied = 8;
        ctx.scope().put(&deck)?; Ok(())
    }).unwrap();
    let count = || meguri_engine::sched::queue::QueueProvider::default().queue(&h.collection, Some(home)).unwrap().value.counts.new;
    assert_eq!(count(), 2);
    custom_study::custom(&h.collection, home, request(Mode::IncreaseNew, 5)).unwrap();
    assert_eq!(count(), 7);
    custom_study::custom(&h.collection, home, request(Mode::IncreaseNew, -2)).unwrap();
    assert_eq!(count(), 5);
    assert_eq!(custom_study::defaults(&h.collection, home).unwrap().extend_new, 5);
    h.clock.set_now_ms(h.collection.next_day_at().unwrap() * 1000 + 1000);
    custom_study::custom(&h.collection, home, request(Mode::IncreaseNew, 5)).unwrap();
    assert_eq!(count(), 15);
}
