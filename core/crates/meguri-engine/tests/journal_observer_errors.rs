//! Completed-commit isolation and conservative cache/deferred failures.
mod common;
mod session_support;

use meguri_engine::{
    ChangeObserver, CommitPayload, Engine, EngineConfig, EngineError, EventKind, ObservedCommit, OpSource, OpSpec,
    Ports,
    sched::{answer::AnswerCache, queue::QueueProvider},
    session::{ReviewService, SessionEvent, StartResult},
};
use meguri_model::ids::DeckId;
use meguri_sched::queue::rebuild_rules::Action;
use meguri_storage::{Cell, ReadRows, Record, RowKey, StampMode, StorageError, Table};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};
use std::time::Duration;

#[derive(Default)]
struct Keep(Mutex<Option<CommitPayload>>);
impl ChangeObserver for Keep {
    fn name(&self) -> &str {
        "keep-unreadable-view"
    }
    fn observe(&self, c: &ObservedCommit<'_>) -> Result<(), EngineError> {
        *self.0.lock().unwrap() = Some(c.payload.clone());
        Ok(())
    }
}

#[test]
fn postcommit_payload_failure_invalidates_watches_caches_and_deferred_projection_without_a_partial_head() {
    let seed = session_support::Harness::new(1);
    let cid = seed.cards[0];
    seed.engine.close().unwrap();
    let engine = Engine::new(EngineConfig::default()).unwrap();
    let queues = Arc::new(QueueProvider::default());
    let review = ReviewService::new(&engine, queues.clone()).unwrap();
    let keep = Arc::new(Keep::default());
    let ports = Ports::new()
        .observer(queues.clone())
        .generation_listener(queues.clone())
        .open_step(review.clone())
        .observer(review.clone())
        .generation_listener(review.clone())
        .suspend_participant(review.clone())
        .observer(keep.clone());
    let c = engine
        .open_raw(
            &seed.dir.path().join("collection.anki2"),
            common::open_config(&seed.clock).ports(ports),
        )
        .unwrap();
    let StartResult::Started {
        session_id,
        head: before,
        ..
    } = review.start(DeckId(1), None).unwrap()
    else {
        panic!("session")
    };
    review.application_barrier_for_testing().unwrap();
    let watch = queues.queue(&c, None).unwrap();
    let cache = AnswerCache::build_scenario_held(&c).unwrap();
    let events = engine.stream().subscribe();
    seed.clock.step_ms(1000);
    let result = c
        .run(
            OpSpec::named("postcommit payload fault", OpSource::UserUndoable),
            move |ctx| {
                let mut card = ctx.scope().card(cid)?.unwrap();
                card.flags = 3;
                ctx.scope().put(&card)?;
                // No payload reads occur until final metadata preparation: one card
                // pair and one finally stamped col pair. The next read is post-COMMIT.
                ctx.scope().journal().unwrap().fail_after_image_reads(4);
                Ok(())
            },
        )
        .unwrap();
    review.application_barrier_for_testing().unwrap();
    assert_eq!(watch.validity.action(), Action::Rebuild);
    assert!(!cache.is_current(&c));
    let view = keep.0.lock().unwrap().take().unwrap();
    assert!(matches!(view.card(cid), Err(StorageError::Io { .. })));
    assert_eq!(result.changes.cards[0].id, cid);
    let events = common::drain(&events, Duration::from_millis(100));
    assert_eq!(common::change_seqs(&events), [result.seq.0]);
    assert!(
        !events.iter().any(|event| {
            event.cause_seq == Some(result.seq)
                && matches!(&event.kind,
            EventKind::Extension(payload) if payload.as_any().downcast_ref::<SessionEvent>().is_some_and(|e|
                matches!(e, SessionEvent::QueueHeadChanged { .. })))
        }),
        "failed payload cannot publish a successful partial head"
    );
    view.fail_image_reads(false);
    assert_eq!(view.card(cid).unwrap().unwrap().after_card().unwrap().unwrap().flags, 3);
    let head = review.head(session_id, 3).unwrap();
    assert_eq!(head.items[0].card_id, before.items[0].card_id);
    assert_eq!(head.items[0].flag, 3, "explicit refresh follows the rebuild path");
    engine.close().unwrap();
}

struct TypedReader(Arc<AtomicUsize>);
impl ChangeObserver for TypedReader {
    fn name(&self) -> &str {
        "typed-reader"
    }
    fn observe(&self, commit: &ObservedCommit<'_>) -> Result<(), EngineError> {
        self.0.fetch_add(1, Ordering::Relaxed);
        for id in commit.payload.revlog_added_ids() {
            commit.payload.revlog_added(id)?;
        }
        for key in &commit.changes.config_keys {
            commit.payload.config(&key.key)?;
        }
        Ok(())
    }
}

#[test]
fn typed_revlog_and_config_errors_are_distinct_from_absence_and_do_not_stop_remaining_observers() {
    for config in [false, true] {
        let calls = Arc::new(AtomicUsize::new(0));
        let keep = Arc::new(Keep::default());
        let h = common::Harness::open_with(
            Ports::new()
                .observer(Arc::new(TypedReader(calls.clone())))
                .observer(keep.clone()),
        );
        let (_, cid) = common::add_note_with_card(&h.collection, OpSource::UserUndoable, &["front", "back"]).value;
        let prior = calls.load(Ordering::Relaxed);
        let result = h
            .collection
            .run(
                OpSpec::named("typed observer failure", OpSource::SyncIncoming).stamp(StampMode::Raw),
                move |ctx| {
                    if config {
                        let row = vec![
                            Cell::text("typed-fault"),
                            Cell::text("bad usn"),
                            Cell::Integer(0),
                            Cell::Blob(b"1".to_vec()),
                        ];
                        ctx.scope()
                            .restore(Table::Config, &RowKey::text("typed-fault"), Some(&row))?;
                    } else {
                        let mut row = common::revlog(cid).to_cells();
                        row[0] = Cell::Integer(1234);
                        row[3] = Cell::text("bad button");
                        ctx.scope().restore(Table::Revlog, &RowKey::Id(1234), Some(&row))?;
                    }
                    Ok(())
                },
            )
            .unwrap();
        assert_eq!(calls.load(Ordering::Relaxed), prior + 1);
        let view = keep.0.lock().unwrap().take().unwrap();
        assert_eq!(view.seq(), result.seq);
        if config {
            assert!(matches!(view.config("typed-fault"), Err(StorageError::Row(_))));
            assert!(view.config("absent").unwrap().is_none());
        } else {
            assert!(matches!(
                view.revlog_added(meguri_model::ids::RevlogId(1234)),
                Err(StorageError::Row(_))
            ));
            assert!(
                view.revlog_removed(meguri_model::ids::RevlogId(1234))
                    .unwrap()
                    .is_none()
            );
        }
    }
}
