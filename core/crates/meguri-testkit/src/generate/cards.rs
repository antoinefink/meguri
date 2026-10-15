//! A card and its review log from its answer instants
//! (`fixtures/synthetic-collections` F5): the history comes first, and the
//! card's scheduling fields and `data` follow from it as Anki's scheduler
//! with the generated presets would have written them (`model/row-records`
//! R4, R5; `model/card-data-json`).

use meguri_model::card::{Card, CardQueue, CardType};
use meguri_model::card_data::{CardData, MemoryState};
use meguri_model::ids::{CardId, DeckId, NoteId, RevlogId};
use meguri_model::revlog::{RevlogEntry, RevlogKind};
use meguri_model::time::EpochSecs;
use meguri_model::usn::Usn;

use super::profile::RevlogShape;
use super::revlog::{Delay, Ending, Fsrs, Phase, difficulty_factor, outcomes};
use super::rng::Rng;
use super::{DESIRED_RETENTION, GenError, Ids, Timeline};

/// When a review card falls due (F5.5 due targets).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Due {
    /// As its interval gives.
    Any,
    /// Today or earlier: the interval is shortened to reach today at most.
    Today,
    /// After today: the interval is lengthened past today at least.
    Later,
}

/// The state a card ends in (F5.2).
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

/// `cards.factor` of a card the moment it first reaches review (F5.4).
pub const STARTING_FACTOR: u16 = 2500;

/// The lowest `cards.factor` after Again or Hard (F5.4).
pub const MINIMUM_FACTOR: u16 = 1300;

/// The most `revlog.time` holds: the presets cap answer times at 60 s.
pub const MAX_ANSWER_MS: i64 = 60_000;

pub struct CardInput<'a> {
    pub id: CardId,
    pub note_id: NoteId,
    pub deck_id: DeckId,
    pub ord: u16,
    /// The note's new-queue position.
    pub position: u32,
    pub created_ms: i64,
    pub target: Target,
    /// Answer instants, increasing, after `id`, before the pinned day.
    pub reviews: &'a [i64],
}

/// What one answer scheduled, after the day-boundary rule (F5.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Scheduled {
    Secs(i64),
    /// A delay in seconds that reached the next day.
    OneDay,
    Days(u32),
}

impl Scheduled {
    /// `revlog.ivl` (F5.6).
    fn revlog_ivl(self) -> i32 {
        match self {
            Scheduled::Secs(secs) => -(secs as i32),
            Scheduled::OneDay => 1,
            Scheduled::Days(days) => days as i32,
        }
    }
}

/// F5.4: `cards.factor` after an answer given in phase `before` that left
/// the card in `after`.
fn next_factor(factor: u16, before: Phase, after: Phase, button: u8) -> u16 {
    match before {
        Phase::Step1 | Phase::Step2 if after == Phase::Review && factor == 0 => STARTING_FACTOR,
        Phase::Review => match button {
            1 => factor.saturating_sub(200).max(MINIMUM_FACTOR),
            2 => factor.saturating_sub(150).max(MINIMUM_FACTOR),
            4 => factor.saturating_add(150),
            _ => factor,
        },
        _ => factor,
    }
}

/// The card row and its review-log rows.
pub fn card_rows(
    shape: &RevlogShape,
    timeline: &Timeline,
    fsrs: &Fsrs,
    input: &CardInput<'_>,
    revlog_ids: &mut Ids,
    rng: &mut Rng,
) -> Result<(Card, Vec<RevlogEntry>), GenError> {
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
        return Ok((card, Vec::new()));
    }
    let wanted = match input.target {
        Target::Learning => Ending::Learning,
        Target::Relearning => Ending::Relearning,
        _ => Ending::Review,
    };
    let (answers, ending) = outcomes(shape, wanted, input.reviews.len(), rng);

    // Review-log ids: each instant, moved past the previous id of the card
    // (its own id first) and any id already taken (F4.7).
    let mut ids = Vec::with_capacity(input.reviews.len());
    let mut previous = input.id.0;
    for &instant in input.reviews {
        let id = revlog_ids.claim(instant.max(previous + 1));
        ids.push(id);
        previous = id;
    }
    let days: Vec<i64> = ids.iter().map(|&id| timeline.day_of(id)).collect();
    let history: Vec<(u8, i64)> = answers.iter().zip(&days).map(|(a, &d)| (a.button, d)).collect();
    let states = fsrs.states(&history)?;
    if states.len() != answers.len() {
        return Err(GenError::Fsrs(format!(
            "{} memory states for {} answers",
            states.len(),
            answers.len()
        )));
    }

    // What each answer scheduled (F5.1).
    let mut scheduled: Vec<Scheduled> = answers
        .iter()
        .zip(&ids)
        .zip(&days)
        .zip(&states)
        .map(|(((answer, &id), &day), state)| match answer.delay {
            Delay::Secs(secs) => {
                let next_day_ms = (timeline.crt + (day + 1) * 86_400) * 1000;
                if id + secs * 1000 >= next_day_ms {
                    Scheduled::OneDay
                } else {
                    Scheduled::Secs(secs)
                }
            }
            Delay::Days => Scheduled::Days(fsrs.interval(state.stability)),
        })
        .collect();

    let last = answers.len() - 1;
    let last_ms = ids[last];
    let last_secs = last_ms.div_euclid(1000);
    let last_day = days[last];
    let last_state = states[last];
    let last_phase = answers[last].after;
    debug_assert!(matches!(
        (ending, last_phase),
        (Ending::Learning, Phase::Step1 | Phase::Step2)
            | (Ending::Relearning, Phase::Relearning)
            | (Ending::Review, Phase::Review)
    ));

    // F5.5: the card's columns from its last answer.
    let learning_due = |card: &mut Card, scheduled: Scheduled| match scheduled {
        Scheduled::Secs(secs) => {
            card.queue = CardQueue::Learning;
            card.due = i32::try_from(last_secs + secs).unwrap_or(i32::MAX);
        }
        _ => {
            card.queue = CardQueue::InterdayLearning;
            card.due = i32::try_from(last_day + 1).unwrap_or(i32::MAX);
        }
    };
    match last_phase {
        Phase::Step1 | Phase::Step2 => {
            card.card_type = CardType::Learning;
            learning_due(&mut card, scheduled[last]);
            card.interval = 0;
            card.remaining_steps = if last_phase == Phase::Step1 { 2 } else { 1 };
        }
        Phase::Relearning => {
            card.card_type = CardType::Relearning;
            learning_due(&mut card, scheduled[last]);
            card.interval = fsrs.interval(last_state.stability);
            card.remaining_steps = 1;
        }
        Phase::Review => {
            let natural = match scheduled[last] {
                Scheduled::Days(days) => i64::from(days),
                _ => 1,
            };
            let interval = match input.target {
                Target::Review(Due::Today) => natural.min(timeline.today - last_day).max(1),
                Target::Review(Due::Later) => natural.max(timeline.today - last_day + 1),
                _ => natural,
            }
            .clamp(1, 36_500);
            card.card_type = CardType::Review;
            card.queue = match input.target {
                Target::Suspended => CardQueue::Suspended,
                Target::Buried => CardQueue::BuriedByUser,
                _ => CardQueue::Review,
            };
            card.interval = interval as u32;
            card.due = i32::try_from(last_day + interval).unwrap_or(i32::MAX);
            card.remaining_steps = 0;
            scheduled[last] = Scheduled::Days(interval as u32);
        }
    }
    let mut factor = 0u16;
    let mut before = Phase::Step1;
    for answer in &answers {
        factor = next_factor(factor, before, answer.after, answer.button);
        before = answer.after;
    }
    card.ease_factor = factor;
    card.reps = answers.len() as u32;
    card.lapses = answers
        .iter()
        .filter(|a| a.button == 1 && a.kind == RevlogKind::Review)
        .count() as u32;
    card.mtime = EpochSecs(last_secs);
    card.data.new_position = Some(input.position);
    card.data.memory_state = Some(MemoryState {
        stability: last_state.stability,
        difficulty: last_state.difficulty,
    });
    card.data.desired_retention = Some(DESIRED_RETENTION);
    card.data.decay = Some(fsrs.decay());
    card.data.last_review_secs = Some(last_secs);

    // F5.6: the review log.
    let mut entries = Vec::with_capacity(ids.len());
    let mut last_interval = 0i32;
    for (i, answer) in answers.iter().enumerate() {
        let interval = scheduled[i].revlog_ivl();
        let time = shape.answer_ms.sample(rng).unwrap_or(8000).clamp(0, MAX_ANSWER_MS);
        entries.push(RevlogEntry {
            id: RevlogId(ids[i]),
            card_id: input.id,
            usn: Usn::PENDING,
            button: answer.button,
            interval,
            last_interval,
            ease_factor: difficulty_factor(states[i].difficulty),
            answer_millis: time as u32,
            kind: answer.kind,
        });
        last_interval = interval;
    }
    Ok((card, entries))
}
