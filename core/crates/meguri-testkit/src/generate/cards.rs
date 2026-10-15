//! A card and its review log from its review instants: the history comes
//! first, and the card's scheduling fields and `data` follow from it
//! (`model/row-records` R4, R5; `model/card-data-json`).

use meguri_model::card::{Card, CardQueue, CardType};
use meguri_model::card_data::{CardData, MemoryState};
use meguri_model::ids::{CardId, DeckId, NoteId, RevlogId};
use meguri_model::revlog::{RevlogEntry, RevlogKind};
use meguri_model::time::EpochSecs;
use meguri_model::usn::Usn;

use super::profile::RevlogShape;
use super::revlog::{Ending, Fsrs, difficulty_factor, outcomes};
use super::rng::Rng;
use super::{DESIRED_RETENTION, Ids, Timeline};

/// When a review card falls due.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Due {
    /// As its interval gives.
    Any,
    /// Today or earlier: the interval is shortened to reach today at most.
    /// Its last review must be before today.
    Today,
    /// After today: the interval is lengthened past today at least.
    Later,
}

/// The state a card ends in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    New,
    Learning,
    Relearning,
    Review(Due),
    Suspended,
    Buried,
}

impl Target {
    pub fn is_new(self) -> bool {
        self == Target::New
    }
}

/// Learning and relearning cards are due again this long after their last
/// review.
pub const STEP_SECS: i64 = 600;

/// `cards.factor` of a card that has left the new queue.
pub const STARTING_FACTOR: u16 = 2500;

pub struct CardInput<'a> {
    pub id: CardId,
    pub note_id: NoteId,
    pub deck_id: DeckId,
    pub ord: u16,
    /// The note's new-queue position.
    pub position: u32,
    pub created_ms: i64,
    pub target: Target,
    /// Review instants, increasing, after `id`.
    pub reviews: &'a [i64],
}

/// The card row and its review-log rows.
pub fn card_rows(
    shape: &RevlogShape,
    timeline: &Timeline,
    fsrs: &Fsrs,
    input: &CardInput<'_>,
    revlog_ids: &mut Ids,
    rng: &mut Rng,
) -> (Card, Vec<RevlogEntry>) {
    let mut card = Card {
        id: input.id,
        note_id: input.note_id,
        deck_id: input.deck_id,
        ord: input.ord,
        mtime: EpochSecs(input.created_ms.div_euclid(1000)),
        usn: Usn::PENDING,
        card_type: CardType::New,
        queue: CardQueue::New,
        due: i32::try_from(input.position).unwrap_or(i32::MAX),
        interval: 0,
        ease_factor: 0,
        reps: 0,
        lapses: 0,
        remaining_steps: 0,
        original_due: 0,
        original_deck_id: DeckId(0),
        flags: 0,
        data: CardData::default(),
    };
    if input.target.is_new() || input.reviews.is_empty() {
        return (card, Vec::new());
    }
    let wanted = match input.target {
        Target::Learning => Ending::Learning,
        Target::Relearning => Ending::Relearning,
        _ => Ending::Review,
    };
    let (buttons, ending) = outcomes(shape, wanted, input.reviews.len(), rng);

    // Review-log ids: each instant, moved past the previous id and any id
    // already taken.
    let mut ids = Vec::with_capacity(input.reviews.len());
    let mut previous = input.id.0;
    for &instant in input.reviews {
        let id = revlog_ids.claim(instant.max(previous + 1));
        ids.push(id);
        previous = id;
    }
    let days: Vec<i64> = ids.iter().map(|&id| timeline.day_of(id)).collect();
    let history: Vec<(u8, i64)> = buttons.iter().zip(&days).map(|(&(b, _), &d)| (b, d)).collect();
    let states = fsrs.states(&history);
    let last_ms = *ids.last().expect("a reviewed card has reviews");
    let last_secs = last_ms.div_euclid(1000);
    let last_day = *days.last().expect("a reviewed card has reviews");
    let stability = states.as_ref().and_then(|s| s.last()).map(|s| s.stability);

    let final_interval = match ending {
        Ending::Learning => {
            card.card_type = CardType::Learning;
            card.queue = CardQueue::Learning;
            card.due = i32::try_from(last_secs + STEP_SECS).unwrap_or(i32::MAX);
            card.remaining_steps = 1;
            -(STEP_SECS as i32)
        }
        Ending::Relearning => {
            card.card_type = CardType::Relearning;
            card.queue = CardQueue::Learning;
            card.due = i32::try_from(last_secs + STEP_SECS).unwrap_or(i32::MAX);
            card.interval = stability.map_or(1, |s| fsrs.interval(s));
            card.remaining_steps = 1;
            -(STEP_SECS as i32)
        }
        Ending::Review => {
            let natural = stability.map_or(1, |s| fsrs.interval(s)) as i64;
            let interval = match input.target {
                Target::Review(Due::Today) => natural.min((timeline.today - last_day).max(1)),
                Target::Review(Due::Later) => natural.max(timeline.today - last_day + 1),
                _ => natural,
            }
            .clamp(1, 36_500);
            card.card_type = CardType::Review;
            card.queue = match input.target {
                Target::Suspended => CardQueue::Suspended,
                Target::Buried => CardQueue::BuriedByScheduler,
                _ => CardQueue::Review,
            };
            card.interval = interval as u32;
            card.due = i32::try_from(last_day + interval).unwrap_or(i32::MAX);
            interval as i32
        }
    };
    card.ease_factor = STARTING_FACTOR;
    card.reps = ids.len() as u32;
    card.lapses = buttons
        .iter()
        .filter(|&&(b, kind)| b == 1 && kind == RevlogKind::Review)
        .count() as u32;
    card.mtime = EpochSecs(last_secs);
    card.data.new_position = Some(input.position);
    card.data.memory_state = states.as_ref().and_then(|s| s.last()).map(|s| MemoryState {
        stability: s.stability,
        difficulty: s.difficulty,
    });
    card.data.desired_retention = Some(DESIRED_RETENTION);
    card.data.decay = Some(fsrs.decay());
    card.data.last_review_secs = Some(last_secs);

    let mut entries = Vec::with_capacity(ids.len());
    let mut last_interval = 0i32;
    for i in 0..ids.len() {
        let interval = if i + 1 == ids.len() {
            final_interval
        } else if days[i + 1] > days[i] {
            i32::try_from(days[i + 1] - days[i]).unwrap_or(i32::MAX)
        } else {
            -i32::try_from(((ids[i + 1] - ids[i]) / 1000).max(1)).unwrap_or(i32::MAX)
        };
        let difficulty = states.as_ref().and_then(|s| s.get(i)).map(|s| s.difficulty);
        let answer = shape
            .answer_ms
            .sample(rng)
            .unwrap_or(8000)
            .clamp(0, i64::from(u32::MAX));
        entries.push(RevlogEntry {
            id: RevlogId(ids[i]),
            card_id: input.id,
            usn: Usn::PENDING,
            button: buttons[i].0,
            interval,
            last_interval,
            ease_factor: difficulty.map_or(u32::from(STARTING_FACTOR), difficulty_factor),
            answer_millis: answer as u32,
            kind: buttons[i].1,
        });
        last_interval = interval;
    }
    (card, entries)
}
