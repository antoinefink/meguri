//! Product storage/engine proof, independent arithmetic and boundary facts.
mod common;
use meguri_engine::{OpSource, OpSpec, Ports, sched::queue::QueueProvider};
use meguri_model::{
    card::{CardQueue, CardType},
    clock::Clock,
    ids::DeckId,
};
use meguri_sched::queue::rebuild_rules::Action;
use meguri_storage::{ConfigValue, ReadRows, ReaderRole, StampMode};
use std::sync::Arc;

fn populate(c: &meguri_engine::Collection, states: Vec<(CardQueue, i64, u32)>) -> Vec<meguri_model::ids::CardId> {
    c.run(
        OpSpec::named("U41 fixture", OpSource::Maintenance).stamp(StampMode::Raw),
        move |ctx| {
            let mut ids = Vec::new();
            for (index, (queue, due, reps)) in states.into_iter().enumerate() {
                let nid = ctx.scope().add_note(&common::note(&[&format!("q{index}"), "back"]))?;
                let mut card = common::card(nid, 0);
                card.queue = queue;
                card.due = due as i32;
                card.reps = reps;
                card.card_type = match queue {
                    CardQueue::New => CardType::New,
                    CardQueue::Review => CardType::Review,
                    _ => CardType::Learning,
                };
                ids.push(ctx.scope().add_card(&card)?);
            }
            Ok(ids)
        },
    )
    .unwrap()
    .value
}

fn dump(c: &meguri_engine::Collection) -> Vec<meguri_storage::raw::RawRows> {
    let rows = c.reader(ReaderRole::Query).unwrap();
    let mut out = Vec::new();
    for table in meguri_storage::Table::ALL {
        out.push(
            rows.execute_raw(&format!("SELECT * FROM {} ORDER BY 1,2", table.name()), &[])
                .unwrap(),
        );
    }
    out.push(
        rows.execute_raw("SELECT * FROM sqlite_master ORDER BY type,name", &[])
            .unwrap(),
    );
    out
}

#[test]
fn boundaries_counts_differences_finished_tomorrow_and_read_only_queries() {
    let h = common::fresh();
    let c = &h.collection;
    let provider = QueueProvider::default();
    let now = h.clock.now_ms() / 1000;
    let day = c.check_rollover().unwrap();
    let end = day.next_day_at;
    let ids = populate(
        c,
        vec![
            (CardQueue::Learning, now - 1, 2),
            (CardQueue::Learning, now, 0),
            (CardQueue::Learning, now + 1200, 1),
            (CardQueue::Learning, now + 1500, 1),
            (CardQueue::Learning, end, 1),
            (CardQueue::Learning, end + 1, 1),
            (CardQueue::Preview, now + 1200, 1),
            (CardQueue::Preview, end, 1),
            (CardQueue::Preview, end + 1, 1),
            (CardQueue::New, 1, 0),
            (CardQueue::Review, i64::from(day.today), 2),
        ],
    );
    c.check_rollover().unwrap();
    let before = dump(c);
    let seq = c.last_seq();
    let undo = c.peek_undo(meguri_engine::UndoScope::Global).unwrap();
    let q = provider.queue(c, None).unwrap();
    assert_eq!(
        q.value.presentation().iter().map(|e| e.id).collect::<Vec<_>>(),
        vec![ids[0], ids[1], ids[10], ids[9], ids[2], ids[6]]
    );
    assert_eq!(
        q.value.learning.len(),
        7,
        "later tail retained; beyond boundary excluded"
    );
    assert_eq!(
        (q.value.counts.new, q.value.counts.learning, q.value.counts.review),
        (1, 4, 1)
    );
    let state = provider.deck_study_state(c, None).unwrap();
    assert_eq!(
        (
            state.list_counts.new,
            state.list_counts.learning,
            state.list_counts.review
        ),
        (1, 3, 1)
    );
    assert_eq!(state.difference.learning, -1);
    assert_eq!((state.learning_remaining_today, state.learning_later_today), (7, 5));
    assert_eq!(state.next_learning_due, Some(now + 1200));
    assert_eq!(state.next_learning_ready_at, Some(now));
    assert_eq!(state.learning_remaining, 6, "compatibility includes imported later q1");
    assert_eq!(state.secs_until_next_learn, 60);
    assert!(!state.finished);
    assert!(!state.empty);
    assert!(!state.new_limit_reached && !state.review_limit_reached);
    let _ = provider.deck_counts(c).unwrap();
    let _ = provider.deck_counts(c).unwrap();
    assert_eq!(
        dump(c),
        before,
        "all collection tables, stamps/config and schema unchanged"
    );
    assert_eq!(c.last_seq(), seq, "no write operation/event reserved");
    assert_eq!(c.peek_undo(meguri_engine::UndoScope::Global).unwrap(), undo);
    assert_eq!(q.validity.action(), Action::None);
    h.clock.set_now_ms(end * 1000 + 1000);
    c.check_rollover().unwrap();
    assert_eq!(
        q.validity.action(),
        Action::Rebuild,
        "day invalidates even with no row delta"
    );
}

#[test]
fn exact_count_refresh_and_no_other_day_or_same_mod_whole_collection_cache() {
    let h = common::fresh();
    let c = &h.collection;
    let provider = Arc::new(QueueProvider::default());
    let now = h.clock.now_ms() / 1000;
    populate(
        c,
        vec![
            (CardQueue::Learning, now + 1200, 1),
            (CardQueue::Preview, now + 1202, 1),
        ],
    );
    let first = provider.deck_counts(c).unwrap();
    assert_eq!(first.counts(DeckId(1)).learning, 0);
    assert_eq!(first.next_change_at, now + 1);
    h.clock.step_ms(1000);
    let next = provider.deck_counts(c).unwrap();
    assert_eq!(next.counts(DeckId(1)).learning, 1);
    assert_eq!(next.next_change_at, now + 2);
    h.clock.step_ms(1000);
    assert_eq!(provider.deck_counts(c).unwrap().counts(DeckId(1)).learning, 2);
    // Raw external edits preserve col.mod; engine reports whole_collection.
    let db = meguri_storage::raw::PlainDatabase::open(&h.path).unwrap();
    db.execute("UPDATE cards SET queue=0", &[]).unwrap();
    drop(db);
    assert!(h.engine.resume().unwrap().external_change);
    assert_eq!(provider.deck_counts(c).unwrap().counts(DeckId(1)).new, 2);
    let day = c.check_rollover().unwrap();
    c.run(
        OpSpec::named("tomorrow reviews", OpSource::Maintenance).stamp(StampMode::Raw),
        move |ctx| {
            for mut card in ctx.scope().cards_after(None, 10)? {
                card.queue = CardQueue::Review;
                card.due = day.today as i32 + 1;
                ctx.scope().put(&card)?;
            }
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(provider.deck_counts(c).unwrap().counts(DeckId(1)).review, 0);
    h.clock.step_ms(86400 * 1000);
    assert_eq!(provider.deck_counts(c).unwrap().counts(DeckId(1)).review, 2);
}

fn provider_counts(
    provider: &QueueProvider,
    collection: &meguri_engine::Collection,
    role: ReaderRole,
) -> (meguri_sched::queue::Counts, Option<u32>) {
    match role {
        ReaderRole::Query => (provider.deck_counts(collection).unwrap().counts(DeckId(1)), None),
        ReaderRole::Interactive => {
            let queue = provider.queue(collection, None).unwrap().value;
            assert_eq!(
                queue.presentation().len(),
                (queue.counts.learning + queue.counts.review) as usize
            );
            (queue.counts, Some(queue.today))
        }
        _ => unreachable!(),
    }
}

#[test]
fn reader_wait_across_learning_readiness_refreshes_counts_and_queue_without_commit() {
    use std::{sync::mpsc, thread, time::Duration};

    for role in [ReaderRole::Query, ReaderRole::Interactive] {
        let h = common::fresh();
        let c = &h.collection;
        let provider = Arc::new(QueueProvider::default());
        let now = h.clock.now_ms() / 1000;
        let due = now + if role == ReaderRole::Query { 1200 } else { 1201 };
        populate(c, vec![(CardQueue::Learning, due, 1)]);
        c.check_rollover().unwrap();
        assert_eq!(provider_counts(&provider, c, role).0.learning, 0);
        let before = dump(c);
        let seq = c.last_seq();
        let held = c.reader(role).unwrap();
        let (reached, waiting) = mpsc::channel();
        provider.notify_before_next_reader(reached);
        let requester = {
            let provider = provider.clone();
            let collection = c.clone();
            thread::spawn(move || provider_counts(&provider, &collection, role))
        };
        assert_eq!(waiting.recv_timeout(Duration::from_secs(5)).unwrap(), role);
        h.clock.step_ms(1000);
        assert_eq!(
            c.last_seq(),
            seq,
            "only the injected clock advanced while the reader was held"
        );
        drop(held);
        let (counts, _) = requester.join().unwrap();
        assert_eq!(counts.learning, 1, "{role:?}");
        assert_eq!(c.last_seq(), seq, "readiness requires no write");
        assert_eq!(dump(c), before);
    }
}

#[test]
fn reader_wait_across_rollover_refreshes_day_counts_and_queue_without_empty_writes() {
    use std::{sync::mpsc, thread, time::Duration};

    for role in [ReaderRole::Query, ReaderRole::Interactive] {
        let h = common::fresh();
        let c = &h.collection;
        let provider = Arc::new(QueueProvider::default());
        let day = c.check_rollover().unwrap();
        populate(c, vec![(CardQueue::Review, i64::from(day.today) + 1, 1)]);
        c.check_rollover().unwrap();
        assert_eq!(provider_counts(&provider, c, role).0.review, 0);
        let before = dump(c);
        let seq = c.last_seq();
        let held = c.reader(role).unwrap();
        let (reached, waiting) = mpsc::channel();
        provider.notify_before_next_reader(reached);
        let requester = {
            let provider = provider.clone();
            let collection = c.clone();
            thread::spawn(move || provider_counts(&provider, &collection, role))
        };
        assert_eq!(waiting.recv_timeout(Duration::from_secs(5)).unwrap(), role);
        h.clock.set_now_ms((day.next_day_at + 1) * 1000);
        assert_eq!(c.last_seq(), seq, "no commit crossed the blocked read");
        drop(held);
        let (counts, today) = requester.join().unwrap();
        assert_eq!(counts.review, 1, "{role:?}");
        if role == ReaderRole::Interactive {
            assert_eq!(today, Some(day.today.wrapping_add(1)));
        }
        assert_eq!(dump(c), before, "automatic rollover keeps the empty-write policy");
    }
}

#[test]
fn fresh_legacy_report_at_exact_boundary_serves_queue_and_counts() {
    use meguri_model::clock::{StepClock, UtcOffset};

    let clock = Arc::new(StepClock::new(common::NOW_MS, UtcOffset::UTC, "UTC"));
    let (_dir, path) = common::day::new_collection(&*clock, common::day::Timing::Legacy);
    let (engine, collection) = common::day::open(&path, clock.clone(), Ports::default());
    let day = collection.check_rollover().unwrap();
    let provider = QueueProvider::default();
    let old = provider.queue(&collection, None).unwrap();
    clock.set_now_ms(day.next_day_at * 1000);
    let before = dump(&collection);
    let queue = provider.queue(&collection, None).unwrap();
    assert_eq!(queue.value.today, day.today.wrapping_add(1));
    assert_eq!(queue.value.next_day_at, day.next_day_at);
    assert_eq!(old.validity.action(), Action::Rebuild);
    assert_eq!(
        queue.validity.action(),
        Action::None,
        "the fresh new-day queue is current"
    );
    assert_eq!(
        provider.deck_counts(&collection).unwrap().counts(DeckId(1)),
        Default::default()
    );
    let report = collection.check_rollover().unwrap();
    assert_eq!(report.today, queue.value.today);
    assert_eq!(
        report.next_day_at,
        clock.now_ms() / 1000,
        "D4's actual boundary is retained"
    );
    assert_eq!(
        queue.validity.action(),
        Action::None,
        "later empty checks keep the fresh proof"
    );
    assert_eq!(
        dump(&collection),
        before,
        "maintenance reserves seqs without writing empty bookkeeping"
    );
    clock.step_ms(999);
    assert_eq!(
        queue.validity.action(),
        Action::None,
        "the proof covers the boundary second"
    );
    clock.step_ms(1);
    assert_eq!(
        queue.validity.action(),
        Action::Rebuild,
        "the proof cannot cover a later second"
    );
    let later = provider.queue(&collection, None).unwrap();
    assert_eq!(later.value.today, queue.value.today);
    assert_eq!(later.value.next_day_at, day.next_day_at + 86400);
    assert_eq!(later.validity.action(), Action::None);
    assert_eq!(old.validity.action(), Action::Rebuild);
    engine.close().unwrap();
}

#[test]
fn fresh_legacy_boundary_validity_checks_clock_inputs_dirty_state_and_generation() {
    use meguri_model::clock::{FixedClock, StepClock, UtcOffset};

    for change in ["offset", "zone", "raw", "marker", "timing", "generation"] {
        let clock = Arc::new(StepClock::new(common::NOW_MS, UtcOffset::UTC, "UTC"));
        let (_dir, path) = common::day::new_collection(&*clock, common::day::Timing::Legacy);
        let (engine, collection) = common::day::open(&path, clock.clone(), Ports::default());
        let boundary = collection.check_rollover().unwrap().next_day_at;
        clock.set_now_ms(boundary * 1000);
        let provider = QueueProvider::default();
        let queue = provider.queue(&collection, None).unwrap();
        assert_eq!(queue.validity.action(), Action::None, "{change}: fresh boundary proof");
        let mut current = collection.clone();
        // Keep the instant fixed: each invalidation must come from its
        // clock input, dirty maintenance basis, or generation alone.
        match change {
            "offset" => clock.set_utc_offset(UtcOffset::from_seconds(3600).unwrap()),
            "zone" => clock.set_zone("Etc/UTC"),
            "raw" => collection.drop_cached_day().unwrap(),
            "marker" | "timing" => {
                let (key, value) = if change == "marker" {
                    ("lastUnburied", i64::from(queue.value.today) + 1)
                } else {
                    ("rollover", 6)
                };
                collection
                    .run(
                        OpSpec::named("boundary clock input", OpSource::Maintenance).stamp(StampMode::Raw),
                        move |ctx| {
                            ctx.scope().set_config_keeping_col_mod(key, &ConfigValue::from(value))?;
                            Ok(())
                        },
                    )
                    .unwrap();
            }
            "generation" => {
                let initial = FixedClock::new(common::NOW_MS, UtcOffset::UTC);
                let (_replacement_dir, replacement) =
                    common::day::new_collection(&initial, common::day::Timing::Legacy);
                let slot = engine.acquire_slot(meguri_engine::LongOp::Restore).unwrap();
                let mut exclusive = slot.enter_exclusive(true).unwrap();
                current = exclusive
                    .replace(move |live| {
                        std::fs::rename(replacement, live)
                            .map_err(|e| meguri_engine::EngineError::Internal { message: e.to_string() })
                    })
                    .unwrap();
            }
            _ => unreachable!(),
        }
        assert_eq!(clock.now_ms(), boundary * 1000);
        assert_eq!(
            queue.validity.action(),
            Action::Rebuild,
            "{change}: old proof is invalid"
        );
        let fresh = provider.queue(&current, None).unwrap();
        assert_eq!(
            fresh.validity.action(),
            Action::None,
            "{change}: fresh inputs are proved"
        );
        if change == "generation" {
            assert_eq!(fresh.value.today, queue.value.today);
            assert_eq!(fresh.value.next_day_at, queue.value.next_day_at);
            assert_ne!(fresh.stamp.generation, queue.stamp.generation);
        }
        engine.close().unwrap();
    }
}

#[test]
fn grouped_inputs_remain_coherent_while_writer_commits() {
    let h = common::fresh();
    let c = &h.collection;
    let now = h.clock.now_ms() / 1000;
    let day = c.check_rollover().unwrap();
    populate(c, vec![(CardQueue::New, 1, 0)]);
    let reader = c.reader(ReaderRole::Query).unwrap();
    let snapshot = reader.snapshot().unwrap();
    let before = snapshot
        .sched_deck_aggregates(day.today, now, day.next_day_at, 1200)
        .unwrap();
    populate(c, vec![(CardQueue::New, 2, 0)]);
    let held = snapshot
        .sched_deck_aggregates(day.today, now, day.next_day_at, 1200)
        .unwrap();
    assert_eq!(before.iter().map(|g| g.today.new).sum::<u32>(), 1);
    assert_eq!(held.iter().map(|g| g.today.new).sum::<u32>(), 1);
    drop(snapshot);
    drop(reader);
    assert_eq!(
        QueueProvider::default().deck_counts(c).unwrap().counts(DeckId(1)).new,
        2
    );
}

#[test]
fn zero_limits_with_no_eligible_cards_are_not_limit_reached_and_no_today_instant() {
    let h = common::fresh();
    let c = &h.collection;
    let provider = QueueProvider::default();
    let day = c.check_rollover().unwrap();
    c.run(OpSpec::named("limits", OpSource::Maintenance), |ctx| {
        let mut deck = ctx.scope().deck(DeckId(1))?.unwrap();
        if let meguri_model::proto::deck::DeckKind::Normal(n) = &mut deck.kind.kind {
            n.new_limit = Some(0);
            n.review_limit = Some(0);
        }
        ctx.scope().put(&deck)?;
        Ok(())
    })
    .unwrap();
    populate(
        c,
        vec![
            (CardQueue::Learning, day.next_day_at + 1, 1),
            (CardQueue::Preview, day.next_day_at + 2, 1),
            (CardQueue::Review, i64::from(day.today) + 2, 1),
        ],
    );
    let s = provider.deck_study_state(c, None).unwrap();
    assert!(s.finished);
    assert_eq!(s.learning_remaining_today, 0);
    assert_eq!(s.next_learning_due, None);
    assert_eq!(s.next_learning_ready_at, None);
    assert!(!s.new_limit_reached && !s.review_limit_reached);
    assert_eq!(s.learning_remaining, 1);
    populate(
        c,
        vec![(CardQueue::New, 1, 0), (CardQueue::Review, i64::from(day.today), 1)],
    );
    let s = provider.deck_study_state(c, None).unwrap();
    assert!(s.new_limit_reached && s.review_limit_reached);
}

#[test]
fn custom_initializers_keep_fnv_and_u82a_on_writer_new_readers_and_reopen() {
    use meguri_storage::{Initializers, rusqlite::functions::FunctionFlags};
    let h = common::fresh();
    h.engine.close().unwrap();
    let mut custom = Initializers::new();
    custom.register(|conn| {
        conn.create_scalar_function(
            "custom_u41",
            0,
            FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_DETERMINISTIC | FunctionFlags::SQLITE_INNOCUOUS,
            |_| Ok(41),
        )
    });
    let options = || common::open_config(&h.clock).initializers(custom.clone());
    for _ in 0..2 {
        let c = h.engine.open_raw(&h.path, options()).unwrap();
        let sql = "SELECT custom_u41(),fnvhash(-1,5),meguri_original_position('{\"pos\":7}'),meguri_retrievability('{}',1,1,1,1),meguri_relative_retrievability('{}',1,1,1,1)";
        let sql = sql.to_owned();
        let writer = c
            .submit_maintenance(
                meguri_engine::executor::catalog::ANALYZE,
                std::time::Duration::from_millis(10),
                move |w| Ok(w.execute_raw(&sql, &[])?),
            )
            .wait()
            .unwrap();
        assert_eq!(writer.rows[0][0], meguri_storage::raw::SqlValue::Integer(41));
        assert_eq!(
            writer.rows[0][1],
            meguri_storage::raw::SqlValue::Integer(-2510909584869376200)
        );
        for role in [ReaderRole::Query, ReaderRole::Interactive, ReaderRole::Maintenance] {
            let r = c.reader(role).unwrap();
            assert_eq!(
                r.execute_raw("SELECT custom_u41(),fnvhash(),meguri_original_position('{}')", &[])
                    .unwrap()
                    .rows[0],
                vec![
                    meguri_storage::raw::SqlValue::Integer(41),
                    meguri_storage::raw::SqlValue::Integer(-3750763034362895579),
                    meguri_storage::raw::SqlValue::Null
                ]
            );
            for arg in ["NULL", "1.0", "'1'", "x'00'"] {
                assert!(r.execute_raw(&format!("SELECT fnvhash({arg})"), &[]).is_err());
            }
            assert!(
                r.execute_raw("SELECT meguri_retrievability('{}',1,1,1,1,1)", &[])
                    .is_err()
            );
            assert!(r.execute_raw("SELECT name FROM sqlite_master WHERE lower(sql) LIKE '%fnvhash%' OR lower(sql) LIKE '%meguri_retrievability%'",&[]).unwrap().rows.is_empty());
        }
        h.engine.close().unwrap();
    }
}

#[test]
fn real_observer_classifies_flags_used_tag_renames_memory_and_external_changes() {
    let clock = Arc::new(meguri_model::clock::StepClock::new(
        common::NOW_MS,
        meguri_model::clock::UtcOffset::UTC,
        "UTC",
    ));
    let (_dir, path) = common::day::new_collection(clock.as_ref(), common::day::Timing::New);
    let provider = Arc::new(QueueProvider::default());
    let (engine, c) = common::day::open(
        &path,
        clock,
        Ports::new()
            .observer(provider.clone())
            .generation_listener(provider.clone()),
    );
    let id = populate(&c, vec![(CardQueue::New, 1, 0)])[0];
    let q = provider.queue(&c, None).unwrap();
    c.run(
        OpSpec::named("flags", OpSource::UserUndoable).stamp(StampMode::KeepMtime),
        move |ctx| {
            let mut card = ctx.scope().card(id)?.unwrap();
            card.flags = 3;
            ctx.scope().put(&card)?;
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(q.validity.action(), Action::None);
    let nid = {
        let r = c.reader(ReaderRole::Query).unwrap();
        r.card(id).unwrap().unwrap().note_id
    };
    c.run(OpSpec::named("note tags", OpSource::UserUndoable), move |ctx| {
        let mut note = ctx.scope().note(nid)?.unwrap();
        note.tags = " old ".into();
        ctx.scope().put(&note)?;
        Ok(())
    })
    .unwrap();
    let q = provider.queue(&c, None).unwrap();
    meguri_engine::tags::rename(&c, "old", "new", &Default::default()).unwrap();
    assert_eq!(q.validity.action(), Action::Patch, "real used rename changes note rows");
    let q = provider.queue(&c, None).unwrap();
    c.run(OpSpec::named("memory", OpSource::Optimizer), move |ctx| {
        let mut card = ctx.scope().card(id)?.unwrap();
        card.data.memory_state = Some(meguri_model::card_data::MemoryState {
            stability: 5.0,
            difficulty: 6.0,
        });
        ctx.scope().put(&card)?;
        Ok(())
    })
    .unwrap();
    assert_eq!(q.validity.action(), Action::Rebuild);
    let q = provider.queue_with_ownership(&c, None, Arc::new(|_| true)).unwrap();
    let db = meguri_storage::raw::PlainDatabase::open(&path).unwrap();
    db.execute("UPDATE cards SET due=2", &[]).unwrap();
    drop(db);
    assert!(engine.resume().unwrap().external_change);
    assert_eq!(
        q.validity.action(),
        Action::Rebuild,
        "whole_collection wins over claimed ownership"
    );
}

#[test]
fn tomorrow_unburies_zeroes_studied_counters_expires_overrides_and_empty_uses_home() {
    let h = common::fresh();
    let c = &h.collection;
    let provider = QueueProvider::default();
    let day = c.check_rollover().unwrap();
    c.run(
        OpSpec::named("projection inputs", OpSource::Maintenance).stamp(StampMode::Raw),
        move |ctx| {
            ctx.scope()
                .set_config("lastUnburied", &ConfigValue::from(u64::from(day.today)))?;
            let mut deck = ctx.scope().deck(DeckId(1))?.unwrap();
            deck.common.last_day_studied = day.today;
            deck.common.new_studied = 20;
            deck.common.review_studied = 200;
            if let meguri_model::proto::deck::DeckKind::Normal(n) = &mut deck.kind.kind {
                n.new_limit_today = Some(meguri_model::proto::deck::DayLimit::new(0, day.today));
            }
            ctx.scope().put(&deck)?;
            for (index, queue, kind, due) in [
                (0, CardQueue::BuriedByScheduler, CardType::New, 1),
                (1, CardQueue::BuriedByScheduler, CardType::Review, day.today as i32 + 1),
                (2, CardQueue::BuriedByUser, CardType::Learning, day.today as i32 + 1),
                (3, CardQueue::BuriedByUser, CardType::Learning, day.next_day_at as i32),
                (4, CardQueue::Suspended, CardType::New, 1),
            ] {
                let nid = ctx
                    .scope()
                    .add_note(&common::note(&[&format!("projection{index}"), "back"]))?;
                let mut card = common::card(nid, 0);
                card.queue = queue;
                card.card_type = kind;
                card.due = due;
                ctx.scope().add_card(&card)?;
            }
            Ok(())
        },
    )
    .unwrap();
    let before = dump(c);
    let s = provider.deck_study_state(c, None).unwrap();
    assert!(s.finished);
    assert!(!s.empty);
    assert_eq!((s.sibling_buried, s.user_buried), (2, 2));
    assert_eq!((s.tomorrow.new, s.tomorrow.learning, s.tomorrow.review), (1, 2, 1));
    assert_eq!(dump(c), before, "projection never performs rollover");
    h.clock.set_now_ms(day.next_day_at * 1000 + 1000);
    let tomorrow = provider.deck_counts(c).unwrap();
    assert_eq!(tomorrow.counts(DeckId(1)), s.tomorrow);
    let child = meguri_engine::decks::add(c, "Home".into()).unwrap().value.id;
    c.run(
        OpSpec::named("borrowed membership", OpSource::Maintenance),
        move |ctx| {
            let card = ctx.scope().cards_after(None, 1)?.remove(0);
            let mut card = card;
            card.original_deck_id = child;
            ctx.scope().put(&card)?;
            Ok(())
        },
    )
    .unwrap();
    let s = provider.deck_study_state(c, Some(child)).unwrap();
    assert!(!s.empty);
    assert!(s.finished);
    assert_eq!(s.queue_counts, Default::default());
}

#[test]
fn damaged_parent_excludes_subtree_and_missing_preset_is_uncapped() {
    let h = common::fresh();
    let c = &h.collection;
    let provider = QueueProvider::default();
    let parent = meguri_engine::decks::add(c, "Missing".into()).unwrap().value.id;
    let child = meguri_engine::decks::add(c, "Missing::Child".into()).unwrap().value.id;
    let id = populate(c, vec![(CardQueue::New, 1, 0)])[0];
    c.run(
        OpSpec::named("damage", OpSource::Maintenance).stamp(StampMode::Raw),
        move |ctx| {
            let mut card = ctx.scope().card(id)?.unwrap();
            card.deck_id = child;
            ctx.scope().put(&card)?;
            ctx.scope().remove_deck(parent)?;
            Ok(())
        },
    )
    .unwrap();
    assert!(provider.queue(c, Some(child)).unwrap().value.presentation().is_empty());
    assert!(provider.deck_counts(c).unwrap().nodes.iter().all(|n| n.id != child));
    c.run(OpSpec::named("missing preset", OpSource::Maintenance), move |ctx| {
        let mut deck = ctx.scope().deck(DeckId(1))?.unwrap();
        if let meguri_model::proto::deck::DeckKind::Normal(n) = &mut deck.kind.kind {
            n.config_id = 999;
            n.new_limit = Some(0);
            n.review_limit = Some(0);
        }
        ctx.scope().put(&deck)?;
        let mut card = ctx.scope().card(id)?.unwrap();
        card.deck_id = DeckId(1);
        ctx.scope().put(&card)?;
        Ok(())
    })
    .unwrap();
    assert_eq!(provider.queue(c, None).unwrap().value.counts.new, 1);
}

#[test]
fn skipped_siblings_propagate_home_bury_flags_limit_refused_siblings_do_not_write_or_propagate() {
    let h = common::fresh();
    let c = &h.collection;
    let provider = QueueProvider::default();
    let day = c.check_rollover().unwrap();
    let now = h.clock.now_ms() / 1000;
    let a = meguri_engine::decks::add(c, "P::A".into()).unwrap().value.id;
    let b = meguri_engine::decks::add(c, "P::B".into()).unwrap().value.id;
    let d = meguri_engine::decks::add(c, "P::C".into()).unwrap().value.id;
    let p = {
        let r = c.reader(ReaderRole::Query).unwrap();
        r.deck_rows()
            .unwrap()
            .into_iter()
            .find(|d| d.human_name() == "P")
            .unwrap()
            .id
    };
    let ids = c
        .run(
            OpSpec::named("bury inputs", OpSource::Maintenance).stamp(StampMode::Raw),
            move |ctx| {
                for (i, did, flags) in [
                    (10, a, [false, false, true]),
                    (11, b, [true, true, false]),
                    (12, d, [false, false, false]),
                ] {
                    let mut preset = ctx.scope().deck_config(meguri_model::ids::DeckConfigId(1))?.unwrap();
                    preset.id = meguri_model::ids::DeckConfigId(i);
                    preset.name = format!("U41-{i}");
                    preset.config.bury_new = flags[0];
                    preset.config.bury_reviews = flags[1];
                    preset.config.bury_interday_learning = flags[2];
                    ctx.scope().put(&preset)?;
                    let mut deck = ctx.scope().deck(did)?.unwrap();
                    if let meguri_model::proto::deck::DeckKind::Normal(n) = &mut deck.kind.kind {
                        n.config_id = i;
                    }
                    ctx.scope().put(&deck)?;
                }
                let nid = ctx.scope().add_note(&common::note(&["siblings", "back"]))?;
                let mut ids = Vec::new();
                for (index, did, queue, due) in [
                    (0, a, CardQueue::Learning, now),
                    (1, b, CardQueue::InterdayLearning, i64::from(day.today)),
                    (2, d, CardQueue::Review, i64::from(day.today)),
                    (3, d, CardQueue::New, 1),
                ] {
                    let mut card = common::card(nid, index);
                    card.deck_id = did;
                    card.queue = queue;
                    card.card_type = CardType::Learning;
                    card.due = due as i32;
                    card.reps = 1;
                    ids.push(ctx.scope().add_card(&card)?);
                }
                Ok(ids)
            },
        )
        .unwrap()
        .value;
    let before = dump(c);
    let q = provider.queue(c, Some(p)).unwrap();
    assert_eq!(
        q.value.presentation().iter().map(|e| e.id).collect::<Vec<_>>(),
        vec![ids[0]]
    );
    assert_eq!(dump(c), before, "gather burial is not persistent");
    c.run(OpSpec::named("refuse interday", OpSource::Maintenance), move |ctx| {
        let mut deck = ctx.scope().deck(b)?.unwrap();
        if let meguri_model::proto::deck::DeckKind::Normal(n) = &mut deck.kind.kind {
            n.review_limit = Some(0);
        }
        ctx.scope().put(&deck)?;
        Ok(())
    })
    .unwrap();
    let q = provider.queue(c, Some(p)).unwrap();
    assert_eq!(
        (q.value.counts.new, q.value.counts.learning, q.value.counts.review),
        (1, 1, 1)
    );
    assert_eq!(q.value.remaining_limits[&b].remaining.review, 0);
}

#[test]
fn independent_three_level_count_arithmetic_and_parent_limits() {
    let h = common::fresh();
    let c = &h.collection;
    let provider = QueueProvider::default();
    let day = c.check_rollover().unwrap();
    let root = meguri_engine::decks::add(c, "root".into()).unwrap().value.id;
    let one = meguri_engine::decks::add(c, "root::one".into()).unwrap().value.id;
    let leaf = meguri_engine::decks::add(c, "root::one::leaf".into()).unwrap().value.id;
    let two = meguri_engine::decks::add(c, "root::two".into()).unwrap().value.id;
    c.run(
        OpSpec::named("count tree inputs", OpSource::Maintenance).stamp(StampMode::Raw),
        move |ctx| {
            for (did, new, review) in [(root, 5, 8), (one, 3, 3), (leaf, 1, 200), (two, 20, 200)] {
                let mut deck = ctx.scope().deck(did)?.unwrap();
                if let meguri_model::proto::deck::DeckKind::Normal(n) = &mut deck.kind.kind {
                    n.new_limit = Some(new);
                    n.review_limit = Some(review);
                }
                ctx.scope().put(&deck)?;
            }
            for (did, new, review) in [(root, 0, 1), (one, 0, 2), (leaf, 3, 2), (two, 4, 3)] {
                for index in 0..new + review {
                    let nid = ctx
                        .scope()
                        .add_note(&common::note(&[&format!("{}-{index}", did.0), "back"]))?;
                    let mut card = common::card(nid, 0);
                    card.deck_id = did;
                    if index >= new {
                        card.queue = CardQueue::Review;
                        card.card_type = CardType::Review;
                        card.due = day.today as i32;
                    }
                    ctx.scope().add_card(&card)?;
                }
            }
            Ok(())
        },
    )
    .unwrap();
    let tree = provider.deck_counts(c).unwrap();
    for (did, n, l, r) in [(leaf, 1, 0, 2), (one, 0, 0, 3), (two, 4, 0, 3), (root, 1, 0, 7)] {
        let actual = tree.counts(did);
        assert_eq!((actual.new, actual.learning, actual.review), (n, l, r));
    }
    c.run(OpSpec::named("parent limits", OpSource::Maintenance), |ctx| {
        ctx.scope()
            .set_config("applyAllParentLimits", &ConfigValue::Bool(true))?;
        Ok(())
    })
    .unwrap();
    assert_eq!(provider.queue(c, Some(leaf)).unwrap().value.counts.review, 2);
    let tree = provider.deck_counts(c).unwrap();
    assert_eq!(tree.counts(leaf).review, 2);
    assert_eq!(tree.counts(two).new, 4);
}

#[test]
fn nested_filtered_policy_is_narrow_queue_exclusion_with_list_and_compatibility_membership() {
    let h = common::fresh();
    let c = &h.collection;
    let provider = QueueProvider::default();
    let now = h.clock.now_ms() / 1000;
    let parent = meguri_engine::decks::add(c, "Study".into()).unwrap().value.id;
    let filtered = meguri_engine::decks::add(c, "Study::Filtered".into()).unwrap().value.id;
    c.run(
        OpSpec::named("imported filtered input", OpSource::Maintenance).stamp(StampMode::Raw),
        move |ctx| {
            let mut deck = ctx.scope().deck(filtered)?.unwrap();
            deck.kind = meguri_model::proto::ProtoBlob::new(meguri_model::proto::deck::DeckKindMessage::new(
                meguri_model::proto::deck::DeckKind::Filtered(Default::default()),
            ));
            ctx.scope().put(&deck)?;
            for (i, queue) in [(0, CardQueue::New), (1, CardQueue::Learning)] {
                let nid = ctx
                    .scope()
                    .add_note(&common::note(&[&format!("filtered{i}"), "back"]))?;
                let mut card = common::card(nid, 0);
                card.deck_id = filtered;
                card.original_deck_id = parent;
                card.queue = queue;
                card.card_type = CardType::Learning;
                card.due = now as i32;
                ctx.scope().add_card(&card)?;
            }
            Ok(())
        },
    )
    .unwrap();
    let before = dump(c);
    let q = provider.queue(c, Some(parent)).unwrap();
    assert!(q.value.presentation().is_empty());
    let s = provider.deck_study_state(c, Some(parent)).unwrap();
    assert!(s.finished && !s.empty);
    assert_eq!((s.list_counts.new, s.list_counts.learning), (1, 1));
    assert_eq!(s.queue_counts, Default::default());
    assert_eq!(s.learning_remaining_today, 0);
    assert_eq!(s.learning_remaining, 1);
    assert!(s.new_remaining);
    assert_eq!(dump(c), before);
    let r = c.reader(ReaderRole::Query).unwrap();
    let structure = meguri_engine::decks::tree::tree(&*r).unwrap();
    drop(r);
    let counted = provider.deck_counts(c).unwrap();
    fn plain(nodes: &[meguri_engine::decks::tree::Node]) -> Vec<(DeckId, String, bool, bool, bool)> {
        nodes
            .iter()
            .flat_map(|n| {
                std::iter::once((n.id, n.name.clone(), n.filtered, n.study_collapsed, n.browser_collapsed))
                    .chain(plain(&n.children))
            })
            .collect()
    }
    fn counted_shape(nodes: &[meguri_sched::counts::deck_tree::Node]) -> Vec<(DeckId, String, bool, bool, bool)> {
        nodes
            .iter()
            .flat_map(|n| {
                std::iter::once((n.id, n.name.clone(), n.filtered, n.study_collapsed, n.browser_collapsed))
                    .chain(counted_shape(&n.children))
            })
            .collect()
    }
    assert_eq!(
        plain(&structure),
        counted_shape(&counted.nodes),
        "U28 names, tree order, Default hiding and flags"
    );
}

#[test]
fn ordinary_counts_issue_one_grouped_card_read_and_no_card_images() {
    use meguri_storage::rusqlite::ffi;
    use std::{
        ffi::{CStr, c_int, c_uint, c_void},
        sync::Mutex,
    };
    // SQLite owns neither the context nor the log. The Initializers closure
    // keeps its Arc alive for every registered writer/reader connection.
    unsafe extern "C" fn trace(
        mask: c_uint,
        context: *mut c_void,
        _statement: *mut c_void,
        text: *mut c_void,
    ) -> c_int {
        if mask == ffi::SQLITE_TRACE_STMT as c_uint && !text.is_null() {
            let log = unsafe { &*(context as *const Mutex<Vec<String>>) };
            let sql = unsafe { CStr::from_ptr(text.cast()) }.to_string_lossy().into_owned();
            if let Ok(mut log) = log.lock() {
                log.push(sql);
            }
        }
        0
    }
    let h = common::fresh();
    populate(&h.collection, vec![(CardQueue::New, 1, 0); 100]);
    h.engine.close().unwrap();
    let log = Arc::new(Mutex::new(Vec::<String>::new()));
    let capture = log.clone();
    let mut init = meguri_storage::Initializers::new();
    init.register(move |conn| {
        let result = unsafe {
            ffi::sqlite3_trace_v2(
                conn.handle(),
                ffi::SQLITE_TRACE_STMT as c_uint,
                Some(trace),
                Arc::as_ptr(&capture).cast_mut().cast(),
            )
        };
        assert_eq!(result, ffi::SQLITE_OK);
        Ok(())
    });
    let c = h
        .engine
        .open_raw(&h.path, common::open_config(&h.clock).initializers(init))
        .unwrap();
    c.check_rollover().unwrap();
    log.lock().unwrap().clear();
    let tree = QueueProvider::default().deck_counts(&c).unwrap();
    assert_eq!(tree.counts(DeckId(1)).new, 20);
    let reads = log
        .lock()
        .unwrap()
        .iter()
        .map(|s| s.to_ascii_lowercase())
        .filter(|s| s.contains("from cards") || s.contains("join cards"))
        .collect::<Vec<_>>();
    assert_eq!(reads.len(), 1, "{reads:?}");
    assert!(reads[0].contains("group by did"));
    h.engine.close().unwrap();
}

#[test]
fn generation_replacement_with_same_day_and_colmod_invalidates_cache_and_watches() {
    let h = common::fresh();
    let provider = QueueProvider::default();
    populate(&h.collection, vec![(CardQueue::New, 1, 0)]);
    let old = provider.queue(&h.collection, None).unwrap();
    assert_eq!(provider.deck_counts(&h.collection).unwrap().counts(DeckId(1)).new, 1);
    let colmod = {
        let r = h.collection.reader(ReaderRole::Query).unwrap();
        r.col().unwrap().modified
    };
    let replacement = common::fresh();
    populate(&replacement.collection, vec![(CardQueue::New, 1, 0); 3]);
    replacement
        .collection
        .run(
            OpSpec::named("same mod", OpSource::Maintenance).stamp(StampMode::Raw),
            move |ctx| {
                ctx.scope().set_col_mod(colmod)?;
                Ok(())
            },
        )
        .unwrap();
    replacement.engine.close().unwrap();
    let path = replacement.path.clone();
    let slot = h.engine.acquire_slot(meguri_engine::LongOp::Restore).unwrap();
    let mut exclusive = slot.enter_exclusive(true).unwrap();
    let new = exclusive
        .replace(move |live| {
            std::fs::rename(path, live).map_err(|e| meguri_engine::EngineError::Internal { message: e.to_string() })
        })
        .unwrap();
    drop(exclusive);
    drop(slot);
    assert_eq!(old.validity.action(), Action::Rebuild);
    assert_eq!(provider.deck_counts(&new).unwrap().counts(DeckId(1)).new, 3);
}

#[test]
fn queue_plus_head_uses_real_u45_and_refuses_writer_reentry() {
    let h = common::fresh();
    let c = &h.collection;
    let p = Arc::new(QueueProvider::default());
    populate(c, vec![(CardQueue::New, 1, 0)]);
    let mut cache = meguri_sched::states::fsrs_glue::FsrsCache::default();
    let (q, head) = p.queue_with_head_states(c, None, &mut cache).unwrap();
    assert_eq!(q.value.counts.new, 1);
    assert!(matches!(
        head.unwrap().current,
        meguri_sched::states::engine::State::New { .. }
    ));
    let c2 = c.clone();
    let result = c.run(
        OpSpec::named("writer reentry", OpSource::Maintenance),
        move |_| match p.queue(&c2, None) {
            Err(meguri_engine::EngineError::InvalidArgument { .. }) => Ok(()),
            _ => panic!("query must refuse before leasing a reader or submitting maintenance"),
        },
    );
    assert!(result.is_ok());
}
