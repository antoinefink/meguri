//! Device-journal values. Floats are stored as bits; replay never chooses again.
use crate::{EngineError, sched::answer::AnswerRequest};
use meguri_model::{card::Card, clock::UtcOffset, ids::CardId, time::EpochMillis};
use meguri_sched::{
    answer::Rating,
    model::{card_sched::CardSched, deck_sched::DeckSched, preset::PresetView, sched_config::SchedConfig},
    queue::Kind,
    states::{
        engine::{AnswerState, ReviewState, RevlogPreview, State},
        fsrs_glue::FsrsCache,
        interval_limits::Delay,
        labels::format_seconds,
        learning::LearningState,
    },
};
use meguri_storage::{ReadRows, StorageError};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub(super) fn error(message: impl ToString) -> EngineError {
    EngineError::InvalidArgument {
        message: format!("review journal: {}", message.to_string()),
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) enum Base {
    Row([u8; 32]),
    Predecessor(u64),
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
struct Learning {
    remaining: u32,
    scheduled: u32,
    elapsed: u32,
    memory: Option<[u32; 2]>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
struct Review {
    scheduled: u32,
    elapsed: u32,
    ease: u32,
    lapses: u32,
    leeched: bool,
    memory: Option<[u32; 2]>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
enum FrozenState {
    New(u32),
    Learning(Learning),
    Review(Review),
    Relearning(Learning, Review),
    Preview(u32, bool),
}
fn memory(m: Option<meguri_model::card_data::MemoryState>) -> Option<[u32; 2]> {
    m.map(|m| [m.stability.to_bits(), m.difficulty.to_bits()])
}
fn thaw_memory(m: Option<[u32; 2]>) -> Option<meguri_model::card_data::MemoryState> {
    m.map(|m| meguri_model::card_data::MemoryState {
        stability: f32::from_bits(m[0]),
        difficulty: f32::from_bits(m[1]),
    })
}
impl From<LearningState> for Learning {
    fn from(l: LearningState) -> Self {
        Self {
            remaining: l.remaining_steps,
            scheduled: l.scheduled_secs,
            elapsed: l.elapsed_secs,
            memory: memory(l.memory_state),
        }
    }
}
impl From<Learning> for LearningState {
    fn from(l: Learning) -> Self {
        Self {
            remaining_steps: l.remaining,
            scheduled_secs: l.scheduled,
            elapsed_secs: l.elapsed,
            memory_state: thaw_memory(l.memory),
        }
    }
}
impl From<ReviewState> for Review {
    fn from(r: ReviewState) -> Self {
        Self {
            scheduled: r.scheduled_days,
            elapsed: r.elapsed_days,
            ease: r.ease_factor.to_bits(),
            lapses: r.lapses,
            leeched: r.leeched,
            memory: memory(r.memory_state),
        }
    }
}
impl From<Review> for ReviewState {
    fn from(r: Review) -> Self {
        Self {
            scheduled_days: r.scheduled,
            elapsed_days: r.elapsed,
            ease_factor: f32::from_bits(r.ease),
            lapses: r.lapses,
            leeched: r.leeched,
            memory_state: thaw_memory(r.memory),
        }
    }
}
impl From<State> for FrozenState {
    fn from(s: State) -> Self {
        match s {
            State::Preview { scheduled_secs, finished } => Self::Preview(scheduled_secs, finished),
            State::New { position } => Self::New(position),
            State::Learning(l) => Self::Learning(l.into()),
            State::Review(r) => Self::Review(r.into()),
            State::Relearning { learning, review } => Self::Relearning(learning.into(), review.into()),
        }
    }
}
impl From<FrozenState> for State {
    fn from(s: FrozenState) -> Self {
        match s {
            FrozenState::Preview(scheduled_secs, finished) => Self::Preview { scheduled_secs, finished },
            FrozenState::New(position) => Self::New { position },
            FrozenState::Learning(l) => Self::Learning(l.into()),
            FrozenState::Review(r) => Self::Review(r.into()),
            FrozenState::Relearning(l, r) => Self::Relearning {
                learning: l.into(),
                review: r.into(),
            },
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct FrozenAnswer {
    pub card: i64,
    pub rating: u8,
    pub at: i64,
    pub elapsed: u64,
    pub today: u32,
    pub boundary: i64,
    pub offset: i32,
    pub kind: u8,
    pub cutoff: i64,
    pub base: Base,
    pub context: [u8; 32],
    current: FrozenState,
    chosen: FrozenState,
    days: bool,
    delay: u32,
    left: u32,
    factor: u16,
    revlog: [i64; 4],
    ahead: u32,
}
impl FrozenAnswer {
    pub(crate) fn capture(r: &AnswerRequest, base: Base, context: [u8; 32], offset: UtcOffset, ahead: u32) -> Self {
        Self {
            card: r.card.0,
            rating: r.rating as u8,
            at: r.answered_at.0,
            elapsed: r.elapsed_ms,
            today: r.today,
            boundary: r.next_day_at,
            offset: offset.seconds(),
            kind: match r.from_queue {
                Some(Kind::New) => 0,
                Some(Kind::Review) => 1,
                Some(Kind::InterdayLearning) => 2,
                _ => 3,
            },
            cutoff: r.learning_cutoff.unwrap_or(r.answered_at.whole_secs().0),
            base,
            context,
            current: r.current.into(),
            chosen: r.chosen.state.into(),
            days: matches!(r.chosen.delay, Delay::Days(_)),
            delay: match r.chosen.delay {
                Delay::Seconds(n) | Delay::Days(n) => n,
            },
            left: r.chosen.left,
            factor: r.chosen.factor,
            revlog: [
                i64::from(r.chosen.revlog.interval),
                i64::from(r.chosen.revlog.last_interval),
                i64::from(r.chosen.revlog.kind),
                i64::from(r.chosen.revlog.factor),
            ],
            ahead,
        }
    }
    pub(crate) fn request(&self, digest: [u8; 32]) -> Result<AnswerRequest, EngineError> {
        let delay = if self.days {
            Delay::Days(self.delay)
        } else {
            Delay::Seconds(self.delay)
        };
        Ok(AnswerRequest {
            card: CardId(self.card),
            state_token: digest,
            current: self.current.clone().into(),
            chosen: AnswerState {
                state: self.chosen.clone().into(),
                delay,
                left: self.left,
                factor: self.factor,
                revlog: RevlogPreview {
                    interval: self.revlog[0].try_into().map_err(error)?,
                    last_interval: self.revlog[1].try_into().map_err(error)?,
                    kind: self.revlog[2].try_into().map_err(error)?,
                    factor: self.revlog[3].try_into().map_err(error)?,
                },
                label: format_seconds(delay.label_seconds(), self.ahead),
            },
            rating: match self.rating {
                1 => Rating::Again,
                2 => Rating::Hard,
                3 => Rating::Good,
                4 => Rating::Easy,
                _ => return Err(error("invalid rating")),
            },
            today: self.today,
            next_day_at: self.boundary,
            answered_at: EpochMillis(self.at),
            elapsed_ms: self.elapsed,
            from_queue: Some(match self.kind {
                0 => Kind::New,
                1 => Kind::Review,
                2 => Kind::InterdayLearning,
                3 => Kind::Learning,
                _ => return Err(error("invalid queue kind")),
            }),
            learning_cutoff: Some(self.cutoff),
            session: None,
            cache: None,
        })
    }
}

pub(crate) struct Inputs {
    pub preset: PresetView,
    pub home: meguri_model::deck::Deck,
    pub retention: f32,
    pub memory: Option<meguri_model::card_data::MemoryState>,
    pub last_review: Option<i64>,
    pub short_term: bool,
    pub ahead: u32,
    pub filtered: Option<meguri_model::proto::deck::FilteredDeck>,
}
/// Resolve scheduling inputs without constructing C12.2's semantic context.
pub(crate) fn inputs(
    rows: &impl ReadRows,
    card: &Card,
    boundary: i64,
    models: &mut FsrsCache,
) -> Result<Inputs, EngineError> {
    inputs_without_revlogs(rows, card, boundary, models, &[])
}
/// Project the same U44 inputs before a committed answer's journal is restored.
pub(crate) fn inputs_without_revlogs(
    rows: &impl ReadRows,
    card: &Card,
    boundary: i64,
    models: &mut FsrsCache,
    removed: &[i64],
) -> Result<Inputs, EngineError> {
    let home = rows
        .deck(CardSched::new(card).home_deck())?
        .ok_or_else(|| error("missing home"))?;
    let meguri_model::proto::deck::DeckKind::Normal(normal) = &home.kind.kind else {
        return Err(error("non-normal home"));
    };
    let preset = PresetView::for_config(rows, meguri_model::ids::DeckConfigId(normal.config_id))?;
    let filtered = meguri_sched::states::filtered::options(rows, card)?;
    let preview = filtered.as_ref().is_some_and(|d| !d.reschedule);
    if !preview {
        models.model(&preset)
            .map_err(|_| crate::sched::memory::error(meguri_sched::memory::resolve::MemoryError::UnsupportedParams))?;
    }
    let retention = DeckSched::new(Some(normal), &home.common).retention(preset.fields().desired_retention);
    let mut history = Vec::new();
    if !preview && ((card.card_type != meguri_model::card::CardType::New && card.data.memory_state.is_none())
        || (!removed.is_empty() && card.data.last_review_secs.is_none()))
    {
        rows.visit_fsrs_histories(
            &meguri_storage::queries::revlog_fsrs::HistorySelection::Cards(vec![card.id]),
            |_, found| -> Result<(), StorageError> {
                history.extend(found.iter().filter(|r| !removed.contains(&r.id.0)).cloned());
                Ok(())
            },
        )?;
    }
    let resolved = if preview {
        card.data.memory_state
    } else {
        meguri_sched::memory::resolve::on_the_fly(models, &preset, card, &history, boundary)
            .map_err(crate::sched::memory::error)?
    };
    let last = match card.data.last_review_secs {
        Some(t) => Some(t),
        None if !removed.is_empty() => meguri_sched::memory::resolve::last_review_time(&history),
        None => rows.fsrs_last_review_time(card.id)?,
    };
    let cfg = SchedConfig::new(rows);
    let short_term = cfg.fsrs_short_term_with_steps()?;
    let ahead = cfg.collapse_time()?;
    Ok(Inputs {
        preset,
        home,
        retention,
        memory: resolved,
        last_review: last,
        short_term,
        ahead,
        filtered,
    })
}

/// C12.2's semantic projection deliberately excludes counters, stamps, unknown
/// bytes, queue order/limits and the current chooser distribution.
pub(crate) fn context(rows: &impl ReadRows, card: &Card, inputs: &Inputs) -> Result<[u8; 32], EngineError> {
    let preset = &inputs.preset;
    let home = &inputs.home;
    let meguri_model::proto::deck::DeckKind::Normal(normal) = &home.kind.kind else {
        return Err(error("non-normal home"));
    };
    let retention = inputs.retention;
    let resolved = inputs.memory;
    let last = inputs.last_review;
    let short_term = inputs.short_term;
    let ahead = inputs.ahead;
    let cfg = SchedConfig::new(rows);
    let p = preset.fields();
    let mut projection = serde_json::json!({
        "home": home.id.0, "preset": normal.config_id, "home_dr": normal.desired_retention.map(f32::to_bits),
        "params": preset.parameters().iter().map(|v| v.to_bits()).collect::<Vec<_>>(),
        "learning": p.learn_steps.iter().map(|v| v.to_bits()).collect::<Vec<_>>(),
        "relearning": p.relearn_steps.iter().map(|v| v.to_bits()).collect::<Vec<_>>(),
        "retention": retention.to_bits(), "historical": p.historical_retention.to_bits(), "ignore": p.ignore_revlogs_before_date,
        "ease": p.initial_ease.to_bits(), "maximum": p.maximum_review_interval,
        "leech": [i64::from(p.leech_threshold), i64::from(p.leech_action)], "cap": p.cap_answer_time_to_secs,
        "bury": [p.bury_new, p.bury_reviews, p.bury_interday_learning], "steps": short_term, "ahead": ahead,
        "crt": rows.col()?.created.0, "creation_offset": cfg.creation_offset()?, "rollover": cfg.rollover_hour()?,
        "history_memory": if card.data.memory_state.is_none() { memory(resolved) } else { None },
        "history_last": if card.data.last_review_secs.is_none() { last } else { None },
    });
    // Keep pre-existing normal-session contexts byte-compatible for recovery.
    if let Some(deck) = &inputs.filtered {
        projection["filtered"] = serde_json::json!((deck.reschedule, deck.preview_again_secs,
            deck.preview_hard_secs, deck.preview_good_secs));
    }
    Ok(Sha256::digest(serde_json::to_vec(&projection).map_err(error)?).into())
}

#[cfg(test)]
pub(super) fn test_answer() -> FrozenAnswer {
    use meguri_sched::states::{engine::RevlogPreview, labels::format_seconds};
    let state = State::Review(ReviewState {
        scheduled_days: 3,
        elapsed_days: 0,
        ease_factor: 2.5,
        lapses: 0,
        leeched: false,
        memory_state: Some(meguri_model::card_data::MemoryState {
            stability: 2.0,
            difficulty: 4.0,
        }),
    });
    let r = AnswerRequest {
        card: CardId(3),
        state_token: [1; 32],
        current: State::New { position: 1 },
        chosen: AnswerState {
            state,
            delay: Delay::Days(3),
            left: 0,
            factor: 2500,
            revlog: RevlogPreview {
                interval: 3,
                last_interval: 0,
                kind: 0,
                factor: 433,
            },
            label: format_seconds(259200, 1200),
        },
        rating: Rating::Easy,
        today: 10,
        next_day_at: 1_894_766_400,
        answered_at: EpochMillis(1_894_701_600_000),
        elapsed_ms: 200,
        from_queue: Some(Kind::New),
        learning_cutoff: Some(1_894_701_600),
        session: None,
        cache: None,
    };
    FrozenAnswer::capture(&r, Base::Row([1; 32]), [2; 32], UtcOffset::UTC, 1200)
}

#[cfg(test)]
#[test]
fn preview_journal_round_trip_retains_the_selected_delay_and_zero_factor() {
    let mut answer = test_answer();
    answer.current = FrozenState::Preview(0, false);
    answer.chosen = FrozenState::Preview(60, false);
    answer.days = false;
    answer.delay = 60;
    answer.left = 0;
    answer.factor = 0;
    answer.revlog = [-60, 0, 3, 0];
    let bytes = serde_json::to_vec(&answer).unwrap();
    let decoded: FrozenAnswer = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(decoded, answer);
    let request = decoded.request([0; 32]).unwrap();
    assert_eq!(request.current, State::Preview { scheduled_secs: 0, finished: false });
    assert_eq!(request.chosen.state, State::Preview { scheduled_secs: 60, finished: false });
    assert_eq!((request.chosen.revlog.kind, request.chosen.revlog.factor), (3, 0));
}
