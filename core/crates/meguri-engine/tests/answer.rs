//! U44 real operation, exact journal restoration and failure atomicity.
mod common;
use common::*;
use meguri_engine::{OpSource, OpSpec, UndoScope, sched::answer};
use meguri_model::{
    card::{CardQueue, CardType},
    clock::Clock,
    ids::*,
    proto::ProtoBlob,
};
use meguri_sched::{answer::Rating, states::fsrs_glue::FsrsCache};
use meguri_storage::{ReadRows, ReaderRole, RowKey, Table};

fn setup() -> (Harness, CardId, CardId) {
    let h = fresh();
    let (id, sibling) = h
        .collection
        .run(OpSpec::named("setup", OpSource::UserNonUndoable), |ctx| {
            let scope = ctx.scope();
            let mut config = scope.deck_config(DeckConfigId(1))?.unwrap();
            let mut fields = (*config.config).clone();
            fields.bury_new = true;
            config.config = ProtoBlob::new(fields);
            scope.put(&config)?;
            let nid = scope.add_note(&note(&["front", "back"]))?;
            let id = scope.add_card(&card(nid, 0))?;
            let sibling = scope.add_card(&card(nid, 1))?;
            Ok((id, sibling))
        })
        .unwrap()
        .value;
    (h, id, sibling)
}
fn images(h: &Harness) -> Vec<(Table, Vec<meguri_storage::RowImage>)> {
    let r = h.collection.reader(ReaderRole::Query).unwrap();
    [
        Table::Cards,
        Table::Revlog,
        Table::Decks,
        Table::Notes,
        Table::Tags,
        Table::Col,
        Table::Config,
    ]
    .into_iter()
    .map(|t| {
        let keys = if t == Table::Col {
            vec![RowKey::Id(1)]
        } else if t == Table::Tags {
            r.tag_rows()
                .unwrap()
                .into_iter()
                .map(|v| RowKey::text(&v.name))
                .collect()
        } else if t == Table::Config {
            r.config_keys().unwrap().into_iter().map(|v| RowKey::text(&v)).collect()
        } else {
            r.ids_after(t, None, 10000)
                .unwrap()
                .into_iter()
                .map(RowKey::Id)
                .collect()
        };
        (t, keys.iter().filter_map(|k| r.row_image(t, k).unwrap()).collect())
    })
    .collect()
}
#[test]
fn answer_buries_inserts_marker_and_undo_redo_restores_exact_rows() {
    let (h, id, sibling) = setup();
    h.clock.step_ms(1000);
    let mut fsrs = FsrsCache::default();
    let prediction = answer::predict(&h.collection, id, &mut fsrs, None).unwrap();
    let before = images(&h);
    let result = answer::answer(&h.collection, prediction.answer(Rating::Good, 95_000)).unwrap();
    assert_eq!(result.value.buried, vec![sibling]);
    assert!(!result.value.leech && !result.value.suspended);
    assert_eq!(
        h.collection.peek_undo(UndoScope::Global).unwrap().name(),
        Some("Answer Card")
    );
    {
        let r = h.collection.reader(ReaderRole::Query).unwrap();
        let log = r.revlog_entry(result.value.revlog_id).unwrap().unwrap();
        assert_eq!(
            (log.button, log.interval, log.last_interval, log.answer_millis),
            (3, -600, 0, 60000)
        );
        let written = r.card(id).unwrap().unwrap();
        assert_eq!(
            (written.card_type, written.queue, written.reps),
            (CardType::Learning, CardQueue::Learning, 1)
        );
        assert_eq!(r.card(sibling).unwrap().unwrap().queue, CardQueue::BuriedByScheduler);
        assert_eq!(
            r.config_value("lastUnburied").unwrap(),
            Some(meguri_storage::ConfigValue::from(0u64))
        );
        assert_eq!(r.col().unwrap().modified.0, NOW_MS + 1000);
        let deck = r.deck(DeckId(1)).unwrap().unwrap();
        assert_eq!((deck.common.new_studied, deck.common.milliseconds_studied), (1, 60000));
    }
    let after = images(&h);
    h.collection.undo(UndoScope::Global).unwrap();
    assert_eq!(images(&h), before);
    h.collection.redo(UndoScope::Global).unwrap();
    assert_eq!(images(&h), after);
    h.engine.close().unwrap();
    let reopened = h.engine.open_raw(&h.path, open_config(&h.clock)).unwrap();
    assert_eq!(
        reopened
            .reader(ReaderRole::Query)
            .unwrap()
            .card(sibling)
            .unwrap()
            .unwrap()
            .queue,
        CardQueue::BuriedByScheduler
    );
}

fn canonical_digest(h: &Harness) -> [u8; 32] {
    // All twelve tables, keys and storage classes. No answer-owned key is
    // excluded, including lastUnburied. Prelude maintenance completes first.
    let dump = common::steps::dump(&h.collection);
    let mut image = Vec::new();
    for (table, rows) in dump {
        image.push(meguri_storage::Cell::text(table));
        for row in rows {
            image.push(meguri_storage::Cell::Integer(row.len() as i64));
            for value in row {
                image.push(match value {
                    meguri_storage::raw::SqlValue::Null => meguri_storage::Cell::Null,
                    meguri_storage::raw::SqlValue::Integer(v) => meguri_storage::Cell::Integer(v),
                    meguri_storage::raw::SqlValue::Real(v) => meguri_storage::Cell::Real(v),
                    meguri_storage::raw::SqlValue::Text(v) => meguri_storage::Cell::text(v),
                    meguri_storage::raw::SqlValue::Blob(v) => meguri_storage::Cell::Blob(v),
                });
            }
        }
    }
    meguri_storage::journal::digest(&image)
}
#[test]
fn answer_undo_restores_the_canonical_collection_digest_without_extra_exclusions() {
    let (h, id, _) = setup();
    review(&h, id, 0, 0);
    h.clock.step_ms(2000);
    let p = answer::predict(&h.collection, id, &mut FsrsCache::default(), None).unwrap();
    let before = canonical_digest(&h);
    answer::answer(&h.collection, p.answer(Rating::Again, 95_000)).unwrap();
    assert_ne!(canonical_digest(&h), before);
    h.collection.undo(UndoScope::Global).unwrap();
    assert_eq!(canonical_digest(&h), before);
}
#[test]
fn row_token_conflict_and_log_veto_preserve_committed_images() {
    let (h, id, _) = setup();
    let cache = answer::AnswerCache::build(&h.collection).unwrap().unwrap();
    let mut fsrs = FsrsCache::default();
    let p = answer::predict(&h.collection, id, &mut fsrs, Some(cache.clone())).unwrap();
    // A temporary SQLite trigger vetoes the log write in the real transaction.
    h.collection.submit_maintenance(meguri_engine::executor::catalog::ANALYZE,std::time::Duration::from_millis(10),|writer| {
        writer.execute_raw("CREATE TRIGGER veto_test_log BEFORE INSERT ON revlog BEGIN SELECT RAISE(ABORT,'synthetic log veto'); END",&[])?;
        Ok(())
    }).wait().unwrap();
    let before = images(&h);
    let cached = cache.snapshot();
    assert!(answer::answer(&h.collection, p.answer(Rating::Good, 4000)).is_err());
    assert_eq!(images(&h), before);
    assert_eq!(cache.snapshot(), cached);
    let p = answer::predict(&h.collection, id, &mut fsrs, None).unwrap();
    h.collection
        .run(
            OpSpec::named("advance repetition witness", OpSource::Maintenance).stamp(meguri_storage::StampMode::Raw),
            move |ctx| {
                let mut c = ctx.scope().card(id)?.unwrap();
                c.reps += 1;
                ctx.scope().put(&c)?;
                Ok(())
            },
        )
        .unwrap();
    let before = images(&h);
    assert!(matches!(
        answer::answer(&h.collection, p.answer(Rating::Good, 0)),
        Err(meguri_engine::EngineError::Stale { .. })
    ));
    assert_eq!(images(&h), before);
}

fn raw(h: &Harness, sql: &str) {
    let sql = sql.to_owned();
    h.collection
        .submit_maintenance(
            meguri_engine::executor::catalog::ANALYZE,
            std::time::Duration::from_millis(10),
            move |w| {
                w.execute_raw(&sql, &[])?;
                Ok(())
            },
        )
        .wait()
        .unwrap();
    h.collection.drop_cached_day().unwrap();
}
fn review(h: &Harness, id: CardId, action: i32, threshold: u32) {
    h.collection
        .run(
            OpSpec::named("review setup", OpSource::Maintenance).stamp(meguri_storage::StampMode::Raw),
            move |ctx| {
                let mut c = ctx.scope().card(id)?.unwrap();
                c.card_type = CardType::Review;
                c.queue = CardQueue::Review;
                c.interval = 20;
                c.ease_factor = 2500;
                c.lapses = 7;
                c.reps = 10;
                c.due = 0;
                c.data = meguri_model::card_data::CardData::parse(
                    r#"{"s":10,"d":5,"lrt":1893837600,"cd":"{ \"source\": 7 }","opaque": [ 2, 3 ]}"#,
                );
                ctx.scope().put(&c)?;
                let mut p = ctx.scope().deck_config(DeckConfigId(1))?.unwrap();
                p.config.leech_action = action;
                p.config.leech_threshold = threshold;
                ctx.scope().put(&p)?;
                Ok(())
            },
        )
        .unwrap();
}
#[test]
fn leech_actions_zero_threshold_and_existing_spelling_restore_exactly() {
    for action in [0, 1, 5] {
        for threshold in [0, 8] {
            for existing in [false, true] {
                let (h, id, _) = setup();
                review(&h, id, action, threshold);
                let note_id = h
                    .collection
                    .reader(ReaderRole::Query)
                    .unwrap()
                    .card(id)
                    .unwrap()
                    .unwrap()
                    .note_id;
                if existing {
                    h.collection
                        .run(OpSpec::named("leech setup", OpSource::Maintenance), move |ctx| {
                            meguri_engine::tags::add_in(ctx.scope(), &[note_id], "LEECH")?;
                            Ok(())
                        })
                        .unwrap();
                }
                let p = answer::predict(&h.collection, id, &mut FsrsCache::default(), None).unwrap();
                let before = images(&h);
                let before_note = h
                    .collection
                    .reader(ReaderRole::Query)
                    .unwrap()
                    .row_image(Table::Notes, &RowKey::Id(note_id.0))
                    .unwrap();
                let result = answer::answer(&h.collection, p.answer(Rating::Again, 2000)).unwrap();
                assert!(result.value.leech);
                assert_eq!(result.value.suspended, action != 1);
                {
                    let r = h.collection.reader(ReaderRole::Query).unwrap();
                    let c = r.card(id).unwrap().unwrap();
                    assert_eq!(c.lapses, 8);
                    assert_eq!(c.card_type, CardType::Relearning);
                    assert_eq!(
                        c.queue,
                        if action == 1 {
                            CardQueue::Learning
                        } else {
                            CardQueue::Suspended
                        }
                    );
                    assert_eq!(
                        r.deck_config(DeckConfigId(1)).unwrap().unwrap().config.leech_threshold,
                        threshold
                    );
                    if existing {
                        assert_eq!(r.row_image(Table::Notes, &RowKey::Id(note_id.0)).unwrap(), before_note);
                    } else {
                        let n = r.note(note_id).unwrap().unwrap();
                        assert_eq!(n.tags, " leech ");
                        assert_eq!((n.mtime.0, n.usn.0), (NOW_MS / 1000, -1));
                    }
                    assert!(c.data.to_text().contains(r#""cd":"{ \"source\": 7 }""#));
                }
                let after = images(&h);
                h.collection.undo(UndoScope::Global).unwrap();
                assert_eq!(images(&h), before);
                h.collection.redo(UndoScope::Global).unwrap();
                assert_eq!(images(&h), after);
            }
        }
    }
}
#[test]
fn marker_update_no_op_no_sibling_and_rollback_share_the_answer_journal() {
    for value in [None, Some("-1"), Some("0")] {
        let (h, id, _) = setup();
        if let Some(value) = value {
            raw(
                &h,
                &format!("INSERT OR REPLACE INTO config VALUES ('lastUnburied',7,19,cast('{value}' as blob))"),
            );
        }
        let p = answer::predict(&h.collection, id, &mut FsrsCache::default(), None).unwrap();
        let before = images(&h);
        let marker_before = h
            .collection
            .reader(ReaderRole::Query)
            .unwrap()
            .row_image(Table::Config, &RowKey::text("lastUnburied"))
            .unwrap();
        let result = answer::answer(&h.collection, p.answer(Rating::Good, 0)).unwrap();
        if value == Some("0") {
            assert_eq!(
                h.collection
                    .reader(ReaderRole::Query)
                    .unwrap()
                    .row_image(Table::Config, &RowKey::text("lastUnburied"))
                    .unwrap(),
                marker_before
            );
            assert!(!result.changes.config_keys.iter().any(|k| k.key == "lastUnburied"));
        } else {
            assert!(result.changes.config_keys.iter().any(|k| k.key == "lastUnburied"));
        }
        h.collection.undo(UndoScope::Global).unwrap();
        assert_eq!(images(&h), before);
    }
    let (h, id, sibling) = setup();
    raw(&h, &format!("DELETE FROM cards WHERE id={}", sibling.0));
    let p = answer::predict(&h.collection, id, &mut FsrsCache::default(), None).unwrap();
    let out = answer::answer(&h.collection, p.answer(Rating::Good, 0)).unwrap();
    assert!(out.value.buried.is_empty());
    assert!(
        h.collection
            .reader(ReaderRole::Query)
            .unwrap()
            .config_entry("lastUnburied")
            .unwrap()
            .is_none()
    );
    // Fail after every answer-owned write, including marker/tag/counters/col.
    let (h, id, _) = setup();
    review(&h, id, 0, 8);
    raw(
        &h,
        "CREATE TRIGGER fail_final BEFORE UPDATE ON col BEGIN SELECT RAISE(ABORT,'final write failed'); END",
    );
    h.clock.step_ms(1000);
    let p = answer::predict(&h.collection, id, &mut FsrsCache::default(), None).unwrap();
    let before = images(&h);
    assert!(answer::answer(&h.collection, p.answer(Rating::Again, 1234)).is_err());
    assert_eq!(images(&h), before);
}
#[test]
fn taken_review_log_instant_uses_maximum_and_keeps_received_kinds_and_bytes() {
    let (h, id, _) = setup();
    for (index, kind) in (0..=5).enumerate() {
        raw(
            &h,
            &format!(
                "INSERT INTO revlog VALUES ({},{},17,4,-2147483648,4294967295,9001,19,{kind})",
                NOW_MS + index as i64 * 1000,
                id.0
            ),
        );
    }
    let old = {
        let r = h.collection.reader(ReaderRole::Query).unwrap();
        r.ids_after(Table::Revlog, None, 100)
            .unwrap()
            .into_iter()
            .map(|id| r.row_image(Table::Revlog, &RowKey::Id(id)).unwrap().unwrap())
            .collect::<Vec<_>>()
    };
    let p = answer::predict(&h.collection, id, &mut FsrsCache::default(), None).unwrap();
    let result = answer::answer(&h.collection, p.answer(Rating::Good, 95_000)).unwrap();
    assert_eq!(result.value.revlog_id.0, NOW_MS + 5001);
    h.collection.undo(UndoScope::Global).unwrap();
    let r = h.collection.reader(ReaderRole::Query).unwrap();
    let after = r
        .ids_after(Table::Revlog, None, 100)
        .unwrap()
        .into_iter()
        .map(|id| r.row_image(Table::Revlog, &RowKey::Id(id)).unwrap().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(old, after);
}
#[test]
fn actual_cache_identity_append_undo_no_redo_rebuild_and_collection_binding() {
    let (h, id, _) = setup();
    review(&h, id, 1, 8);
    let cache = answer::AnswerCache::build(&h.collection).unwrap().unwrap();
    let built = cache.snapshot();
    assert_eq!(built.entries(DeckConfigId(1), 0).len(), 1);
    let p = answer::predict(&h.collection, id, &mut FsrsCache::default(), Some(cache.clone())).unwrap();
    let mut request = p.answer(Rating::Good, 2000);
    request.from_queue = Some(meguri_sched::queue::Kind::Review);
    let result = answer::answer(&h.collection, request).unwrap();
    let interval = h
        .collection
        .reader(ReaderRole::Query)
        .unwrap()
        .card(id)
        .unwrap()
        .unwrap()
        .interval;
    assert_eq!(result.value.metadata.cache_build, Some(cache.identity()));
    assert_eq!(cache.snapshot().entries(DeckConfigId(1), 0).len(), 1);
    assert_eq!(cache.snapshot().entries(DeckConfigId(1), interval).len(), 1);
    let rebuilt = answer::AnswerCache::build(&h.collection).unwrap().unwrap();
    let rebuilt_rows = rebuilt.snapshot();
    assert_ne!(rebuilt.identity(), cache.identity());
    h.collection.undo(UndoScope::Global).unwrap();
    assert!(cache.snapshot().entries(DeckConfigId(1), 0).is_empty());
    assert!(cache.snapshot().entries(DeckConfigId(1), interval).is_empty());
    assert_eq!(rebuilt.snapshot(), rebuilt_rows);
    let undone = cache.snapshot();
    h.collection.redo(UndoScope::Global).unwrap();
    assert_eq!(cache.snapshot(), undone);
    let other = fresh();
    assert!(!cache.is_current(&other.collection));
    cache.invalidate();
    assert!(!cache.is_current(&h.collection));
    let generation = h.collection.generation();
    h.engine.close().unwrap();
    let reopened = h.engine.open_raw(&h.path, open_config(&h.clock)).unwrap();
    assert_ne!(generation, reopened.generation());
    assert!(!rebuilt.is_current(&reopened));
}
#[test]
fn missing_preset_uses_factory_and_invalid_deck_filter_and_preset_refuse_atomically() {
    let (h, id, _) = setup();
    raw(&h, "DELETE FROM deck_config WHERE id=1");
    let p = answer::predict(&h.collection, id, &mut FsrsCache::default(), None).unwrap();
    assert_eq!(
        p.states.good.delay,
        meguri_sched::states::interval_limits::Delay::Seconds(600)
    );
    let out = answer::answer(&h.collection, p.answer(Rating::Good, 0)).unwrap();
    assert!(!out.value.leech);
    for damage in ["DELETE FROM decks WHERE id=1", "UPDATE cards SET odid=1"] {
        let (h, id, _) = setup();
        let mut request = answer::predict(&h.collection, id, &mut FsrsCache::default(), None)
            .unwrap()
            .answer(Rating::Good, 0);
        raw(&h, damage);
        request.state_token = h
            .collection
            .reader(ReaderRole::Query)
            .unwrap()
            .row_digest(Table::Cards, &RowKey::Id(id.0))
            .unwrap()
            .unwrap();
        let before = images(&h);
        assert!(answer::answer(&h.collection, request).is_err());
        assert_eq!(images(&h), before);
    }
    let (h, id, _) = setup();
    let request = answer::predict(&h.collection, id, &mut FsrsCache::default(), None)
        .unwrap()
        .answer(Rating::Good, 0);
    h.collection
        .run(OpSpec::named("unsupported", OpSource::Maintenance), |ctx| {
            let mut p = ctx.scope().deck_config(DeckConfigId(1))?.unwrap();
            p.config.set_fsrs_params_6(vec![1.0; 22]);
            ctx.scope().put(&p)?;
            Ok(())
        })
        .unwrap();
    let before = images(&h);
    assert!(answer::answer(&h.collection, request).is_err());
    assert_eq!(images(&h), before);
}
#[test]
fn marker_precedes_burial_and_legacy_creation_offset_stays_absent() {
    let (h, id, sibling) = setup();
    raw(&h, "DELETE FROM config WHERE key IN ('creationOffset','lastUnburied')");
    let day = h.collection.check_rollover().unwrap();
    raw(
        &h,
        &format!(
            "CREATE TRIGGER marker_before_burial BEFORE UPDATE OF queue ON cards WHEN new.queue=-2 AND NOT EXISTS (SELECT 1 FROM config WHERE key='lastUnburied' AND cast(val as text)='{}') BEGIN SELECT RAISE(ABORT,'missing bury marker'); END",
            day.today
        ),
    );
    let p = answer::predict(&h.collection, id, &mut FsrsCache::default(), None).unwrap();
    answer::answer(&h.collection, p.answer(Rating::Good, 0)).unwrap();
    assert_eq!(h.collection.check_rollover().unwrap().today, day.today);
    let r = h.collection.reader(ReaderRole::Query).unwrap();
    assert!(r.config_entry("creationOffset").unwrap().is_none());
    assert_eq!(r.card(sibling).unwrap().unwrap().queue, CardQueue::BuriedByScheduler);
}
#[test]
fn intraday_answers_still_record_capped_time_and_stamp_unchanged_decks() {
    let (h, id, _) = setup();
    let p = answer::predict(&h.collection, id, &mut FsrsCache::default(), None).unwrap();
    answer::answer(&h.collection, p.answer(Rating::Good, 0)).unwrap();
    h.clock.step_ms(1000);
    let p = answer::predict(&h.collection, id, &mut FsrsCache::default(), None).unwrap();
    answer::answer(&h.collection, p.answer(Rating::Again, 95_000)).unwrap();
    {
        let r = h.collection.reader(ReaderRole::Query).unwrap();
        let d = r.deck(DeckId(1)).unwrap().unwrap();
        assert_eq!(
            (
                d.common.new_studied,
                d.common.review_studied,
                d.common.learning_studied,
                d.common.milliseconds_studied
            ),
            (1, 0, 0, 60000)
        );
        assert_eq!((d.mtime.0, d.usn.0), (NOW_MS / 1000 + 1, -1));
    }
    h.clock.step_ms(1000);
    let p = answer::predict(&h.collection, id, &mut FsrsCache::default(), None).unwrap();
    let result = answer::answer(&h.collection, p.answer(Rating::Again, 0)).unwrap();
    assert!(result.changes.decks.iter().any(|d| d.id == DeckId(1)));
    assert_eq!(
        h.collection
            .reader(ReaderRole::Query)
            .unwrap()
            .deck(DeckId(1))
            .unwrap()
            .unwrap()
            .mtime
            .0,
        NOW_MS / 1000 + 2
    );
}

#[test]
fn answer_without_cache_append_undo_removes_build_entry_and_preserves_rebuilt_cache() {
    let (h, id, _) = setup();
    review(&h, id, 1, 9999);
    let cache = answer::AnswerCache::build(&h.collection).unwrap().unwrap();
    let built = cache.snapshot();
    assert_eq!(
        built
            .entries(DeckConfigId(1), 0)
            .iter()
            .filter(|e| e.card == id)
            .count(),
        1
    );
    let mut request = answer::predict(&h.collection, id, &mut FsrsCache::default(), Some(cache.clone()))
        .unwrap()
        .answer(Rating::Again, 0);
    request.from_queue = Some(meguri_sched::queue::Kind::Review);
    let result = answer::answer(&h.collection, request).unwrap();
    assert_eq!(result.value.metadata.cache_build, Some(cache.identity()));
    assert_eq!(
        h.collection
            .reader(ReaderRole::Query)
            .unwrap()
            .card(id)
            .unwrap()
            .unwrap()
            .queue,
        CardQueue::Learning
    );
    assert_eq!(cache.snapshot(), built, "intraday answer appends no entry");
    let rebuilt = answer::AnswerCache::build(&h.collection).unwrap().unwrap();
    let rebuilt_value = rebuilt.snapshot();
    assert_ne!(rebuilt.identity(), cache.identity());
    h.collection.undo(UndoScope::Global).unwrap();
    assert!(
        cache
            .snapshot()
            .entries(DeckConfigId(1), 0)
            .iter()
            .all(|e| e.card != id)
    );
    assert_eq!(rebuilt.snapshot(), rebuilt_value);
    let undone = cache.snapshot();
    h.collection.redo(UndoScope::Global).unwrap();
    assert_eq!(cache.snapshot(), undone);
    assert_eq!(rebuilt.snapshot(), rebuilt_value);
}

#[test]
fn grade_now_undo_and_redo_leave_its_answer_cache_untouched() {
    let (h, id, _) = setup();
    review(&h, id, 0, 0);
    let cache = answer::AnswerCache::build(&h.collection).unwrap().unwrap();
    let before = cache.snapshot();
    let request = answer::predict(&h.collection, id, &mut FsrsCache::default(), Some(cache.clone()))
        .unwrap()
        .answer(Rating::Good, 0);
    assert!(request.from_queue.is_none());
    let result = answer::answer(&h.collection, request).unwrap();
    assert!(result.value.metadata.from_queue.is_none());
    let after = cache.snapshot();
    assert_ne!(after, before, "the committed review really appended");
    h.collection.undo(UndoScope::Global).unwrap();
    assert_eq!(cache.snapshot(), after);
    h.collection.redo(UndoScope::Global).unwrap();
    assert_eq!(cache.snapshot(), after);
}

#[test]
fn zero_time_intraday_answer_stamps_deck_without_canonicalizing_unchanged_blobs() {
    use meguri_model::{proto::BlobMessage, time::EpochSecs, usn::Usn};
    let (h, id, _) = setup();
    let p = answer::predict(&h.collection, id, &mut FsrsCache::default(), None).unwrap();
    answer::answer(&h.collection, p.answer(Rating::Good, 0)).unwrap();
    // Explicit default plus an overlong zero: valid, intentionally noncanonical.
    // The row already belongs to today, so another intraday answer changes no counter.
    raw(
        &h,
        "UPDATE decks SET common=CAST(common || X'088000' AS BLOB), kind=CAST(kind || X'F0018000' AS BLOB), name='  Counter probe  ', usn=27, mtime_secs=1 WHERE id=1",
    );
    let deck = h
        .collection
        .reader(ReaderRole::Query)
        .unwrap()
        .deck(DeckId(1))
        .unwrap()
        .unwrap();
    let bytes = deck.common.to_bytes().into_owned();
    let kind = deck.kind.to_bytes().into_owned();
    let name = deck.name.clone();
    assert_ne!(bytes, deck.common.encode());
    h.clock.step_ms(1000);
    let p = answer::predict(&h.collection, id, &mut FsrsCache::default(), None).unwrap();
    let at = p.answered_at.whole_secs().0;
    answer::answer(&h.collection, p.answer(Rating::Again, 0)).unwrap();
    let saved = h
        .collection
        .reader(ReaderRole::Query)
        .unwrap()
        .deck(DeckId(1))
        .unwrap()
        .unwrap();
    assert_eq!(saved.common.to_bytes().as_ref(), bytes);
    assert_eq!(saved.kind.to_bytes().as_ref(), kind);
    assert_eq!(saved.name, name);
    assert_eq!((saved.mtime, saved.usn), (EpochSecs(at), Usn::PENDING));
    assert_ne!((saved.mtime, saved.usn), (deck.mtime, deck.usn));
    let p = answer::predict(&h.collection, id, &mut FsrsCache::default(), None).unwrap();
    answer::answer(&h.collection, p.answer(Rating::Again, 7)).unwrap();
    let changed = h
        .collection
        .reader(ReaderRole::Query)
        .unwrap()
        .deck(DeckId(1))
        .unwrap()
        .unwrap();
    assert_eq!(
        changed.common.milliseconds_studied,
        saved.common.milliseconds_studied + 7
    );
    assert_ne!(changed.common.to_bytes().as_ref(), bytes);
    assert_eq!(changed.common.to_bytes().as_ref(), changed.common.encode());
    assert_eq!(changed.kind.to_bytes().as_ref(), kind);
    assert_eq!(changed.name, name);
}

#[cfg(feature = "test-hooks")]
#[test]
fn blocked_answer_prediction_and_native_build_refresh_rollover_before_reading() {
    use std::{sync::mpsc, thread};
    for building in [false, true] {
        let (h, id, _) = setup();
        let old = h.collection.check_rollover().unwrap();
        let held = h.collection.reader(ReaderRole::Interactive).unwrap();
        let (reached, waiting) = mpsc::channel();
        answer::AnswerCache::notify_before_next_reader(&h.collection, reached);
        let collection = h.collection.clone();
        let reader = thread::spawn(move || {
            if building {
                (None, answer::AnswerCache::build(&collection).unwrap())
            } else {
                (
                    Some(answer::predict(&collection, id, &mut FsrsCache::default(), None).unwrap()),
                    None,
                )
            }
        });
        assert_eq!(waiting.recv().unwrap(), ReaderRole::Interactive);
        let at = (old.next_day_at + 1) * 1000;
        h.clock.set_now_ms(at);
        drop(held);
        let (prediction, cache) = reader.join().unwrap();
        let current = h.collection.check_rollover().unwrap();
        assert_eq!(current.today, old.today.wrapping_add(1));
        if let Some(cache) = cache {
            assert!(cache.is_current(&h.collection));
            assert_eq!(
                (cache.snapshot().today, cache.next_day_at()),
                (current.today, current.next_day_at)
            );
        }
        let prediction =
            prediction.unwrap_or_else(|| answer::predict(&h.collection, id, &mut FsrsCache::default(), None).unwrap());
        assert_eq!(
            (prediction.today, prediction.next_day_at, prediction.answered_at.0),
            (current.today, current.next_day_at, at)
        );
        answer::answer(&h.collection, prediction.answer(Rating::Easy, 0)).unwrap();
        let rows = h.collection.reader(ReaderRole::Query).unwrap();
        assert_eq!(rows.col().unwrap().modified.0, at);
        assert_eq!(
            rows.deck(DeckId(1)).unwrap().unwrap().common.last_day_studied,
            current.today
        );
        let card = rows.card(id).unwrap().unwrap();
        assert_eq!(card.mtime.0, at / 1000);
        assert_eq!(card.due, current.today as i32 + card.interval as i32);
    }
}

#[cfg(feature = "test-hooks")]
#[test]
fn answer_readers_settle_timing_commit_before_trusting_day_or_snapshot() {
    use meguri_storage::{ConfigValue, StampMode};
    use std::{sync::mpsc, thread};
    enum Progress {
        Waiting(meguri_engine::Seq),
        Returned,
    }
    for building in [false, true] {
        let (h, id, _) = setup();
        let initial = h.collection.check_rollover().unwrap();
        h.clock.set_now_ms((initial.next_day_at + 3600) * 1000);
        let old = h.collection.check_rollover().unwrap();
        let (committed, published) = mpsc::channel();
        let (resume, paused) = mpsc::channel();
        h.collection.after_next_commit(move |seq| {
            let _ = committed.send(seq);
            let _ = paused.recv();
        });
        let collection = h.collection.clone();
        let writer = thread::spawn(move || {
            collection.run(
                OpSpec::named("move answer rollover", OpSource::Maintenance).stamp(StampMode::Raw),
                |ctx| {
                    ctx.scope()
                        .set_config_keeping_col_mod("rollover", &ConfigValue::from(6_i64))?;
                    Ok(())
                },
            )
        });
        let seq = published.recv().unwrap();
        let (progress, events) = mpsc::channel();
        let waiting = progress.clone();
        h.collection.on_next_settled_wait(move |seq| {
            let _ = waiting.send(Progress::Waiting(seq));
        });
        let collection = h.collection.clone();
        let reader = thread::spawn(move || {
            let result = if building {
                let cache = answer::AnswerCache::build(&collection).unwrap().unwrap();
                assert!(cache.is_current(&collection));
                (cache.snapshot().today, cache.next_day_at())
            } else {
                let p = answer::predict(&collection, id, &mut FsrsCache::default(), None).unwrap();
                (p.today, p.next_day_at)
            };
            let _ = progress.send(Progress::Returned);
            result
        });
        assert!(matches!(events.recv().unwrap(), Progress::Waiting(waiting) if waiting == seq));
        assert_eq!(h.collection.check_rollover().unwrap().today, old.today);
        {
            let lease = h.collection.reader(ReaderRole::Interactive).unwrap();
            let snapshot = lease.snapshot().unwrap();
            assert_eq!(snapshot.seq(), seq);
            assert_eq!(
                snapshot.config_value("rollover").unwrap(),
                Some(ConfigValue::from(6_i64))
            );
        }
        assert!(matches!(events.try_recv(), Err(mpsc::TryRecvError::Empty)));
        resume.send(()).unwrap();
        writer.join().unwrap().unwrap();
        let found = reader.join().unwrap();
        let current = h.collection.check_rollover().unwrap();
        assert_eq!(current.today, old.today.wrapping_sub(1));
        assert_eq!(found, (current.today, current.next_day_at));
    }
}

#[test]
fn fresh_legacy_boundary_cache_balances_predictions_for_only_the_checked_second() {
    let (h, id, _) = setup();
    review(&h, id, 0, 0);
    raw(&h, "DELETE FROM config WHERE key='creationOffset'");
    let old = answer::AnswerCache::build(&h.collection).unwrap().unwrap();
    assert!(old.is_current(&h.collection));
    h.clock.set_now_ms(old.next_day_at() * 1000);
    assert!(
        !old.is_current(&h.collection),
        "yesterday's build has no checked-second proof"
    );
    let day = h.collection.check_rollover().unwrap();
    assert_eq!(day.next_day_at, h.clock.now_ms() / 1000);
    let plain = answer::predict(&h.collection, id, &mut FsrsCache::default(), None).unwrap();
    let meguri_sched::states::interval_limits::Delay::Days(plain_hard) = plain.states.hard.delay else {
        panic!("review delay")
    };
    h.collection
        .run(
            OpSpec::named("load the plain candidate", OpSource::Maintenance),
            move |ctx| {
                for _ in 0..64 {
                    let nid = ctx.scope().add_note(&note(&["cache weight probe", "day bucket"]))?;
                    let mut filler = card(nid, 0);
                    filler.card_type = CardType::Review;
                    filler.queue = CardQueue::Suspended;
                    filler.due = day.today.wrapping_add(plain_hard) as i32;
                    ctx.scope().add_card(&filler)?;
                }
                Ok(())
            },
        )
        .unwrap();
    let cache = answer::AnswerCache::build(&h.collection).unwrap().unwrap();
    assert!(cache.is_current(&h.collection));
    assert_eq!(
        (cache.snapshot().today, cache.next_day_at()),
        (day.today, day.next_day_at)
    );
    let balanced = answer::predict(&h.collection, id, &mut FsrsCache::default(), Some(cache.clone())).unwrap();
    assert!(balanced.cache.is_some());
    assert_ne!(
        balanced.states.hard.delay, plain.states.hard.delay,
        "fresh boundary build must offer the real chooser"
    );
    h.clock.step_ms(999);
    assert!(cache.is_current(&h.collection));
    h.clock.step_ms(1);
    assert!(!cache.is_current(&h.collection));
    let later = answer::AnswerCache::build(&h.collection).unwrap().unwrap();
    assert!(later.is_current(&h.collection));
    assert_eq!(later.snapshot().today, day.today);
    assert_eq!(later.next_day_at(), day.next_day_at + 86400);
    assert!(!old.is_current(&h.collection));
    assert!(!cache.is_current(&h.collection));
}

#[test]
fn answer_uses_the_completed_current_report_for_due_and_studied_day() {
    let (h, id, _) = setup();
    let p = answer::predict(&h.collection, id, &mut FsrsCache::default(), None).unwrap();
    let today = p.today;
    let mut request = p.answer(Rating::Easy, 0);
    request.today = today.wrapping_add(91);
    request.next_day_at = 1;
    answer::answer(&h.collection, request).unwrap();
    let rows = h.collection.reader(ReaderRole::Query).unwrap();
    let card = rows.card(id).unwrap().unwrap();
    assert_eq!(card.due, today as i32 + card.interval as i32);
    assert_eq!(rows.deck(DeckId(1)).unwrap().unwrap().common.last_day_studied, today);
}

#[test]
fn boundary_build_proof_rejects_dirty_day_offset_and_zone_changes() {
    use meguri_model::clock::UtcOffset;
    for change in 0..3 {
        let (h, id, _) = setup();
        review(&h, id, 0, 0);
        raw(&h, "DELETE FROM config WHERE key='creationOffset'");
        let day = h.collection.check_rollover().unwrap();
        h.clock.set_now_ms(day.next_day_at * 1000);
        let cache = answer::AnswerCache::build(&h.collection).unwrap().unwrap();
        assert!(cache.is_current(&h.collection));
        match change {
            0 => h.collection.drop_cached_day().unwrap(),
            1 => h.clock.set_utc_offset(UtcOffset::from_seconds(3600).unwrap()),
            _ => h.clock.set_zone("Etc/UTC"),
        }
        assert!(!cache.is_current(&h.collection));
        let fresh = answer::AnswerCache::build(&h.collection).unwrap().unwrap();
        assert!(fresh.is_current(&h.collection));
        assert_ne!(fresh.identity(), cache.identity());
    }
}
