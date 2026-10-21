use super::{
    Counts, Entry, Environment, Kind, LearningEntry, Queue, bury_gather::BuryModes, learning, limits, mix, order,
};
use meguri_model::{card::Card, ids::DeckId};
use meguri_storage::{ReadRows, StorageError, queries::sched_queue::Phase};

pub fn build(rows: &impl ReadRows, env: &Environment, deck: DeckId) -> Result<Queue, StorageError> {
    let selected = env.subtree(deck, true);
    let preset = env.index(deck).and_then(|i| env.preset(i));
    let review_order = preset.map_or(0, |p| {
        if (0..=12).contains(&p.review_order) {
            p.review_order as u8
        } else {
            0
        }
    });
    let mut q = Queue {
        deck,
        main: Vec::new(),
        learning: Vec::new(),
        learning_cutoff: env.now,
        learn_ahead: env.ahead,
        counts: Counts::default(),
        remaining_limits: limits::tree(env, deck, &selected),
        today: env.today,
        next_day_at: env.next_day_at,
        fsrs: env.fsrs,
        review_limit_reached: false,
        new_limit_reached: false,
    };
    let mut bury = BuryModes::default();
    let mut lists = [Vec::<Card>::new(), Vec::new(), Vec::new()];
    for (phase, kind, index) in [
        (Phase::Intraday, Kind::Learning, 0),
        (Phase::Interday, Kind::InterdayLearning, 0),
        (Phase::Review, Kind::Review, 1),
        (Phase::New, Kind::New, 2),
    ] {
        let mut cards =
            rows.sched_queue_candidates(&selected, phase, review_order, env.today, env.now, env.next_day_at)?;
        if kind == Kind::New {
            order::gather(
                &mut cards,
                &selected,
                preset.map_or(0, |p| p.new_card_gather_priority),
                env.today,
            );
        }
        for card in cards {
            if kind != Kind::Learning && !limits::permits(&q.remaining_limits, card.deck_id, kind) {
                if !bury.skips(&card, kind) {
                    if kind == Kind::New {
                        q.new_limit_reached = true;
                    } else {
                        q.review_limit_reached = true;
                    }
                }
                continue;
            }
            if bury.offer(&card, kind, env.bury_preset(&card)) {
                continue;
            }
            if kind == Kind::Learning {
                q.learning.push(LearningEntry {
                    id: card.id,
                    due: i64::from(card.due),
                    reps: card.reps,
                    mtime: card.mtime,
                });
            } else {
                limits::take(&mut q.remaining_limits, card.deck_id, kind);
                lists[index].push(card);
            }
        }
    }
    learning::order(&mut q.learning);
    let [interday, reviews, mut new] = lists;
    order::sort_new(&mut new, preset.map_or(1, |p| p.new_card_sort_order), env.today);
    q.counts = Counts {
        new: new.len() as u32,
        review: reviews.len() as u32,
        learning: interday.len() as u32
            + q.learning
                .iter()
                .filter(|c| c.due <= env.now + i64::from(env.ahead))
                .count() as u32,
    };
    let entries = |cards: Vec<Card>, kind| {
        cards
            .into_iter()
            .map(|c| Entry {
                id: c.id,
                kind,
                mtime: c.mtime,
            })
            .collect()
    };
    let main = mix::mix(
        entries(reviews, Kind::Review),
        entries(interday, Kind::InterdayLearning),
        preset.map_or(0, |p| p.interday_learning_mix),
    );
    q.main = mix::mix(main, entries(new, Kind::New), preset.map_or(0, |p| p.new_mix));
    Ok(q)
}
