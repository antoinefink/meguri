//! Part P uses hand-built queue values, independently of SQL gathering.
use meguri_model::{
    ids::{CardId, DeckId},
    time::EpochSecs,
};
use meguri_sched::{
    queue::{Counts, Entry, Kind, LearningEntry, Queue},
    session::{Requeue, RestoreError, SessionQueue},
};

fn entry(id: i64, due: i64, reps: u32) -> LearningEntry {
    LearningEntry {
        id: CardId(id),
        due,
        reps,
        mtime: EpochSecs(7),
    }
}
fn queue(main: &[i64], learning: Vec<LearningEntry>, learning_count: u32) -> SessionQueue {
    SessionQueue::new(
        Queue {
            deck: DeckId(1),
            main: main
                .iter()
                .map(|id| Entry {
                    id: CardId(*id),
                    kind: Kind::Review,
                    mtime: EpochSecs(7),
                })
                .collect(),
            learning,
            counts: Counts {
                new: 0,
                learning: learning_count,
                review: main.len() as u32,
            },
            learning_cutoff: 100,
            learn_ahead: 120,
            remaining_limits: Default::default(),
            today: 3,
            next_day_at: 1000,
            fsrs: true,
            review_limit_reached: false,
            new_limit_reached: false,
        },
        1,
    )
}
fn ids(q: &mut SessionQueue, now: i64) -> Vec<i64> {
    q.read(now, usize::MAX).iter().map(|e| e.id.0).collect()
}

#[test]
fn cutoff_stays_fixed_until_an_answer_or_an_empty_read() {
    let mut q = queue(&[1, 2], vec![entry(3, 110, 1), entry(4, 300, 1)], 1);
    assert_eq!(ids(&mut q, 200), [1, 2, 3]);
    q.answer(CardId(1), None, &[], 200).unwrap();
    assert_eq!(ids(&mut q, 200), [3, 2, 4]);
    assert_eq!(
        q.value().counts,
        Counts {
            new: 0,
            learning: 2,
            review: 1
        }
    );
}

#[test]
fn held_learning_becomes_available_at_exact_due_minus_ahead() {
    let mut q = queue(&[], vec![entry(1, 600, 1)], 0);
    assert!(ids(&mut q, 479).is_empty());
    assert_eq!(q.waiting_until(), Some(480));
    assert_eq!(ids(&mut q, 480), [1]);
    assert_eq!(q.value().counts.learning, 1);
}

#[test]
fn requeue_front_rule_changes_only_queue_due_and_respects_edge() {
    let mut q = queue(&[], vec![entry(1, 100, 2), entry(2, 150, 1)], 2);
    let record = q
        .answer(
            CardId(1),
            Some(Requeue {
                due: 140,
                reps: 3,
                mtime: EpochSecs(8),
            }),
            &[],
            100,
        )
        .unwrap();
    assert_eq!(q.value().learning[1].due, 151);
    assert_eq!(ids(&mut q, 100), [2, 1]);
    q.undo(&record).unwrap();
    assert_eq!(ids(&mut q, 100), [1, 2]);
    q.redo(&record).unwrap();
    assert_eq!(ids(&mut q, 100), [2, 1]);
    let mut edge = queue(&[], vec![entry(1, 100, 1), entry(2, 219, 1)], 2);
    edge.answer(
        CardId(1),
        Some(Requeue {
            due: 140,
            reps: 2,
            mtime: EpochSecs(8),
        }),
        &[],
        100,
    )
    .unwrap();
    assert_eq!(edge.value().learning[0].due, 140);
}

#[test]
fn unattempted_front_entry_does_not_override_attempted_sort_key() {
    let mut q = queue(&[], vec![entry(1, 100, 1), entry(2, 150, 0)], 2);
    q.answer(
        CardId(1),
        Some(Requeue {
            due: 140,
            reps: 2,
            mtime: EpochSecs(8),
        }),
        &[],
        100,
    )
    .unwrap();
    assert_eq!(q.value().learning[0].due, 151);
    assert_eq!(ids(&mut q, 100), [1, 2]);
}

#[test]
fn learning_anywhere_decrements_with_a_zero_lower_bound() {
    let mut q = queue(&[1, 2], vec![entry(3, 200, 1), entry(4, 500, 1)], 1);
    q.answer(CardId(4), None, &[], 100).unwrap();
    assert_eq!(q.value().counts.learning, 0);
    assert_eq!(ids(&mut q, 100), [1, 2, 3]);
    let before = q.value().counts;
    assert!(q.answer(CardId(2), None, &[], 100).is_err());
    assert_eq!(q.value().counts, before);
    assert_eq!(ids(&mut q, 100), [1, 2, 3]);
}

#[test]
fn redo_restores_pre_advance_cutoff_and_learning_snapshot() {
    let mut q = queue(&[1], vec![entry(2, 150, 1), entry(3, 280, 1)], 1);
    let record = q.answer(CardId(1), None, &[], 200).unwrap();
    assert_eq!(ids(&mut q, 200), [2, 3]);
    q.undo(&record).unwrap();
    assert_eq!(ids(&mut q, 200), [1, 2]);
    q.redo(&record).unwrap();
    assert_eq!(q.value().learning_cutoff, 100);
    assert_eq!(q.value().counts.learning, 1);
    assert_eq!(ids(&mut q, 200), [2]);
}

#[test]
fn queued_siblings_are_removed_and_counted_down() {
    let mut q = queue(&[1, 2, 3], vec![], 0);
    let record = q.answer(CardId(1), None, &[CardId(2)], 100).unwrap();
    assert_eq!(ids(&mut q, 100), [3]);
    assert_eq!(q.value().counts.review, 1);
    q.undo(&record).unwrap();
    assert_eq!(ids(&mut q, 100), [1, 2, 3]);
    q.redo(&record).unwrap();
    assert_eq!(ids(&mut q, 100), [3]);
}

#[test]
fn current_ownership_is_explicit_and_rebuild_requires_a_fresh_restore() {
    let mut q = queue(&[1, 2], vec![], 0);
    q.set_current(Some(CardId(2)));
    assert_eq!(ids(&mut q, 100), [2, 1]);
    let record = q.answer(CardId(2), None, &[], 100).unwrap();
    q.rebuild(queue(&[1, 2], vec![], 0).value().clone(), 2);
    assert_eq!(q.undo(&record), Err(RestoreError::BuildChanged));
    q.set_current(Some(CardId(2)));
    assert_eq!(ids(&mut q, 100), [2, 1]);
}

#[test]
fn prediction_is_the_same_transition_for_ten_thousand_seeded_queues() {
    // A local deterministic integer stream describes queue inputs, not scheduler arithmetic.
    let mut seed = 0x43u64;
    for _ in 0..10_000 {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        let due = 101 + (seed % 400) as i64;
        let now = 100 + ((seed >> 12) % 200) as i64;
        let mut q = queue(&[1, 2], vec![entry(3, due, 1)], u32::from(due <= 220));
        let requeue = Some(Requeue {
            due: now + (seed % 180) as i64,
            reps: 2,
            mtime: EpochSecs(now),
        });
        let predicted = q.predict(CardId(1), requeue, &[], now).unwrap();
        q.answer(CardId(1), requeue, &[], now).unwrap();
        assert_eq!(q.read(now, 1).first().map(|e| e.id), predicted);
    }
}
