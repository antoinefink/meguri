//! Isolated Rust-heap proof for J4/S2. Fixtures precede each measurement.
//! Process/device phys_footprint/RSS acceptance remains separate.
mod common;
mod session_support;

use meguri_engine::{
    ChangeObserver, CommitPayload, Engine, EngineConfig, EngineError, ObservedCommit, OpSource, OpSpec, Ports,
    UndoScope,
    sched::queue::QueueProvider,
    session::{ReviewService, StartResult},
};
use meguri_model::{
    card::CardQueue,
    ids::{CardId, DeckId},
};
use meguri_storage::{Cell, JournalOptions, ReadRows, ReaderRole, Record, RowKey, StampMode, Table};
use meguri_testkit::bench::peak_alloc::PeakAlloc;
use std::sync::{
    Arc, Condvar, Mutex,
    atomic::{AtomicBool, AtomicUsize, Ordering},
    mpsc,
};
use std::time::Duration;

#[global_allocator]
static ALLOC: PeakAlloc = PeakAlloc;
const CARDS: usize = 150_000;
const FIRST: i64 = 10_000_000;
const WAIT: Duration = Duration::from_secs(60);
const LIMIT: usize = JournalOptions::DEFAULT_SPILL_THRESHOLD;

#[derive(Default)]
struct Delivery {
    owner: Mutex<Option<CommitPayload>>,
    after_reads: Mutex<usize>,
    active: AtomicBool,
    tail_ok: AtomicBool,
    classifier_reads: AtomicUsize,
}
impl ChangeObserver for Delivery {
    fn name(&self) -> &str {
        "streaming-real-payload"
    }
    fn observe(&self, commit: &ObservedCommit<'_>) -> Result<(), EngineError> {
        if !self.active.load(Ordering::Acquire) || commit.changes.cards.len() != CARDS {
            return Ok(());
        }
        let reads = commit.payload.image_reads();
        let owner = commit.payload.clone();
        assert_eq!(owner.card_ids().count(), CARDS);
        assert_eq!(reads, owner.image_reads(), "capture and enumeration do not decode");
        assert!(owner.held_image_bytes() <= LIMIT);
        assert!(owner.spilled_image_bytes() > LIMIT as u64);
        let mut count = 0;
        for delta in owner.cards() {
            let delta = delta?;
            assert!(delta.before_card()?.is_some());
            assert!(delta.after_card()?.is_some());
            count += 1;
        }
        assert_eq!(count, CARDS);
        *self.after_reads.lock().unwrap() = owner.image_reads();
        *self.owner.lock().unwrap() = Some(owner);
        Ok(())
    }
}
struct Tail(Arc<Delivery>);
impl ChangeObserver for Tail {
    fn name(&self) -> &str {
        "payload-read-counter"
    }
    fn observe(&self, commit: &ObservedCommit<'_>) -> Result<(), EngineError> {
        if self.0.active.load(Ordering::Acquire) && commit.changes.cards.len() == CARDS {
            // Queue watch proof reads at most its first scheduling-relevant
            // card. Review callback queues an owner; its worker is gated.
            let reads = commit.payload.image_reads() - *self.0.after_reads.lock().unwrap();
            self.0.tail_ok.store(
                reads <= self.0.classifier_reads.load(Ordering::Relaxed),
                Ordering::Release,
            );
        }
        Ok(())
    }
}

fn image(id: i64, note: meguri_model::ids::NoteId, width: usize, value: char) -> meguri_storage::RowImage {
    let mut card = common::card(note, 0);
    card.id = CardId(id);
    card.queue = CardQueue::Suspended;
    let mut image = card.to_cells();
    image[17] = Cell::text(format!(
        "{{\"retained\":\"{}-{id}\"}}",
        value.to_string().repeat(width + id as usize % 17)
    ));
    image
}

fn measured(width: usize) -> usize {
    // Bootstrap a real production review fixture. Only its separate live
    // card is eligible: this measures changed-payload retention and delivery,
    // independently of the pre-existing live queue gather implementation.
    let seed = session_support::Harness::new(1);
    let live_id = seed.cards[0];
    let nid = seed
        .collection
        .run(OpSpec::named("bulk fixture note", OpSource::Maintenance), move |ctx| {
            let mut note = common::note(&["bulk suspended", "back"]);
            let live = ctx.scope().card(live_id)?.unwrap();
            note.notetype_id = ctx.scope().note(live.note_id)?.unwrap().notetype_id;
            Ok(ctx.scope().add_note(&note)?)
        })
        .unwrap()
        .value;
    seed.collection
        .run(
            OpSpec::named("prepare bulk rows", OpSource::Maintenance).stamp(StampMode::Raw),
            move |ctx| {
                for offset in 0..CARDS {
                    let id = FIRST + offset as i64;
                    let row = image(id, nid, width, 'a');
                    ctx.scope().restore(Table::Cards, &RowKey::Id(id), Some(&row))?;
                }
                Ok(())
            },
        )
        .unwrap();
    seed.engine.close().unwrap();
    let engine = Engine::new(EngineConfig::default()).unwrap();
    let queues = Arc::new(QueueProvider::default());
    let review = ReviewService::new(&engine, queues.clone()).unwrap();
    let delivery = Arc::new(Delivery::default());
    let ports = Ports::new()
        .observer(delivery.clone())
        .observer(queues.clone())
        .generation_listener(queues.clone())
        .open_step(review.clone())
        .observer(review.clone())
        .generation_listener(review.clone())
        .suspend_participant(review.clone())
        .observer(Arc::new(Tail(delivery.clone())));
    let path = seed.dir.path().join("collection.anki2");
    let c = engine
        .open_raw(&path, common::open_config(&seed.clock).ports(ports))
        .unwrap();
    let StartResult::Started { session_id, .. } = review.start(DeckId(1), None).unwrap() else {
        panic!("active session required")
    };
    review.application_barrier_for_testing().unwrap();
    let (tx, rx) = mpsc::sync_channel(1);
    let gate = Arc::new((Mutex::new(false), Condvar::new()));
    let worker_gate = gate.clone();
    review.observe_hook_for_testing(Some(Arc::new(move |payload| {
        // No decoding before generation/seq/ownership classification.
        if payload.card_ids().count() != CARDS {
            return;
        }
        let reads = payload.image_reads();
        tx.send((payload.clone(), reads)).unwrap();
        let (open, changed) = &*worker_gate;
        let open = open.lock().unwrap();
        let (open, timed) = changed.wait_timeout_while(open, WAIT, |open| !*open).unwrap();
        assert!(*open && !timed.timed_out());
    })));
    seed.clock.step_ms(1000);
    delivery
        .classifier_reads
        .store(2 * queues.live_watch_count_for_testing(), Ordering::Relaxed);
    delivery.active.store(true, Ordering::Release);
    let baseline = PeakAlloc::current();
    PeakAlloc::reset_peak();
    let result = c
        .run(
            OpSpec::named("measured changed cards", OpSource::UserUndoable).stamp(StampMode::Raw),
            move |ctx| {
                for offset in 0..CARDS {
                    let id = FIRST + offset as i64;
                    let row = image(id, nid, width, 'b');
                    ctx.scope().restore(Table::Cards, &RowKey::Id(id), Some(&row))?;
                }
                Ok(())
            },
        )
        .unwrap();
    let (queued, reads) = rx.recv_timeout(WAIT).unwrap();
    assert_eq!((queued.seq(), queued.generation()), (result.seq, result.generation));
    assert_eq!(reads, queued.image_reads(), "queueing/dequeueing reads no images");
    assert!(queued.held_image_bytes() <= LIMIT);
    assert!(queued.spilled_image_bytes() > LIMIT as u64);
    *gate.0.lock().unwrap() = true;
    gate.1.notify_all();
    review.application_barrier_for_testing().unwrap();
    let extra = PeakAlloc::peak().saturating_sub(baseline);
    assert!(
        delivery.tail_ok.load(Ordering::Acquire),
        "capture decoded the bulk payload"
    );
    delivery.active.store(false, Ordering::Release);
    // Both key counts and compact history/touch/ChangeSet metadata are
    // identical between widths. Only bounded row/codec buffers may scale.
    eprintln!(
        "cards={CARDS} width={width} held={} spill={} heap_growth={extra} reads={}",
        queued.held_image_bytes(),
        queued.spilled_image_bytes(),
        queued.image_reads()
    );
    review.observe_hook_for_testing(None);
    let retained = delivery.owner.lock().unwrap().take().unwrap();
    for _ in 0..2 {
        let delta = retained.card(CardId(FIRST)).unwrap().unwrap();
        assert_eq!(delta.before, Some(image(FIRST, nid, width, 'a')));
        assert_eq!(delta.after, Some(image(FIRST, nid, width, 'b')));
    }
    c.undo(UndoScope::Global).unwrap();
    review.application_barrier_for_testing().unwrap();
    assert_eq!(
        c.reader(ReaderRole::Query)
            .unwrap()
            .row_image(Table::Cards, &RowKey::Id(FIRST))
            .unwrap(),
        Some(image(FIRST, nid, width, 'a'))
    );
    c.redo(UndoScope::Global).unwrap();
    review.application_barrier_for_testing().unwrap();
    assert_eq!(
        c.reader(ReaderRole::Query)
            .unwrap()
            .row_image(Table::Cards, &RowKey::Id(FIRST))
            .unwrap(),
        Some(image(FIRST, nid, width, 'b'))
    );
    c.run(
        OpSpec::named("later conflicting row", OpSource::SyncIncoming),
        move |ctx| {
            let row = image(FIRST, nid, width, 'c');
            ctx.scope().restore(Table::Cards, &RowKey::Id(FIRST), Some(&row))?;
            Ok(())
        },
    )
    .unwrap();
    review.application_barrier_for_testing().unwrap();
    assert!(
        !review.head(session_id, 3).unwrap().items.is_empty(),
        "real worker delivered to an active session"
    );
    engine.close().unwrap();
    for view in [&queued, &retained] {
        for _ in 0..2 {
            let delta = view.card(CardId(FIRST)).unwrap().unwrap();
            assert_eq!(delta.before, Some(image(FIRST, nid, width, 'a')));
            assert_eq!(delta.after, Some(image(FIRST, nid, width, 'b')));
        }
    }
    extra
}

#[test]
fn bounded_both_sides_through_real_observers_and_active_review_at_150k() {
    let narrow = measured(64);
    let wide = measured(2048);
    // The worker gate prevents overlapping decoding during delivery.
    let max_row = 32 * std::mem::size_of::<Cell>() + 2 * (2048 + 17 + 128);
    let max_codec = 2 * max_row + 18 * 9;
    let working = 8 * max_row + 8 * max_codec;
    // Subtracting the widths removes equal key metadata. Reserve one image
    // budget for variation in how rows fit into spill storage.
    assert!(
        wide <= narrow + LIMIT + working,
        "payload heap scales with total row bytes: narrow={narrow}, wide={wide}, held_limit={LIMIT}, working={working}"
    );
}
