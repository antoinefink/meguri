//! Typed C1–C19 boundary. Session algorithms and durability stay in ReviewService.
use crate::{
    callback,
    error::CoreError,
    render::{RenderPlatform, RenderedCard},
    runtime,
};
use meguri_engine::{self as engine, session};
use meguri_model::{
    ids::{CardId, DeckId},
    time::EpochMillis,
};
use meguri_sched::{self as sched, states::engine as states};

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct ReviewStateToken {
    pub bytes: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct ReviewRenderKey {
    pub bytes: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct ReviewMemoryState {
    pub stability: f32,
    pub difficulty: f32,
}

impl From<meguri_model::card_data::MemoryState> for ReviewMemoryState {
    fn from(v: meguri_model::card_data::MemoryState) -> Self {
        Self {
            stability: v.stability,
            difficulty: v.difficulty,
        }
    }
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct ReviewLearningState {
    pub remaining_steps: u32,
    pub scheduled_secs: u32,
    pub elapsed_secs: u32,
    pub memory_state: Option<ReviewMemoryState>,
}

impl From<sched::states::learning::LearningState> for ReviewLearningState {
    fn from(v: sched::states::learning::LearningState) -> Self {
        Self {
            remaining_steps: v.remaining_steps,
            scheduled_secs: v.scheduled_secs,
            elapsed_secs: v.elapsed_secs,
            memory_state: v.memory_state.map(Into::into),
        }
    }
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct ReviewReviewState {
    pub scheduled_days: u32,
    pub elapsed_days: u32,
    pub ease_factor: f32,
    pub lapses: u32,
    pub leeched: bool,
    pub memory_state: Option<ReviewMemoryState>,
}

impl From<states::ReviewState> for ReviewReviewState {
    fn from(v: states::ReviewState) -> Self {
        Self {
            scheduled_days: v.scheduled_days,
            elapsed_days: v.elapsed_days,
            ease_factor: v.ease_factor,
            lapses: v.lapses,
            leeched: v.leeched,
            memory_state: v.memory_state.map(Into::into),
        }
    }
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct ReviewRevlogPreview {
    pub interval: i32,
    pub last_interval: i32,
    pub kind: u8,
    pub factor: u32,
}

impl From<states::RevlogPreview> for ReviewRevlogPreview {
    fn from(v: states::RevlogPreview) -> Self {
        Self {
            interval: v.interval,
            last_interval: v.last_interval,
            kind: v.kind,
            factor: v.factor,
        }
    }
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct ReviewAnswerLabel {
    pub less_than: bool,
    pub amount: String,
    pub unit: ReviewTimeUnit,
}

impl From<sched::states::labels::AnswerLabel> for ReviewAnswerLabel {
    fn from(v: sched::states::labels::AnswerLabel) -> Self {
        Self {
            less_than: v.less_than,
            amount: v.amount,
            unit: v.unit.into(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct ReviewAnswerState {
    pub state: ReviewSchedulingState,
    pub delay: ReviewDelay,
    pub left: u32,
    pub factor: u16,
    pub revlog: ReviewRevlogPreview,
    pub label: ReviewAnswerLabel,
}

impl From<states::AnswerState> for ReviewAnswerState {
    fn from(v: states::AnswerState) -> Self {
        Self {
            state: v.state.into(),
            delay: v.delay.into(),
            left: v.left,
            factor: v.factor,
            revlog: v.revlog.into(),
            label: v.label.into(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct ReviewSchedulingStates {
    pub current: ReviewSchedulingState,
    pub again: ReviewAnswerState,
    pub hard: ReviewAnswerState,
    pub good: ReviewAnswerState,
    pub easy: ReviewAnswerState,
}

impl From<states::SchedulingStates> for ReviewSchedulingStates {
    fn from(v: states::SchedulingStates) -> Self {
        Self {
            current: v.current.into(),
            again: v.again.into(),
            hard: v.hard.into(),
            good: v.good.into(),
            easy: v.easy.into(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct ReviewPredictions {
    pub again: Option<i64>,
    pub hard: Option<i64>,
    pub good: Option<i64>,
    pub easy: Option<i64>,
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct ReviewHomePreset {
    pub id: i64,
    pub autoplay: bool,
    pub skip_question_when_replaying_answer: bool,
    pub wait_for_audio: bool,
    pub show_timer: bool,
    pub stop_timer_on_answer: bool,
    pub maximum_answer_seconds: u32,
    pub question_seconds: f32,
    pub question_action: i32,
    pub answer_seconds: f32,
    pub answer_action: i32,
    pub leech_action: i32,
}

impl From<session::HomePreset> for ReviewHomePreset {
    fn from(v: session::HomePreset) -> Self {
        Self {
            id: v.id.0,
            autoplay: v.autoplay,
            skip_question_when_replaying_answer: v.skip_question_when_replaying_answer,
            wait_for_audio: v.wait_for_audio,
            show_timer: v.show_timer,
            stop_timer_on_answer: v.stop_timer_on_answer,
            maximum_answer_seconds: v.maximum_answer_seconds,
            question_seconds: v.question_seconds,
            question_action: v.question_action,
            answer_seconds: v.answer_seconds,
            answer_action: v.answer_action,
            leech_action: v.leech_action,
        }
    }
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct ReviewHeadItem {
    pub card_id: i64,
    pub note_id: i64,
    pub kind: ReviewQueueKind,
    pub state_token: ReviewStateToken,
    pub render_key: ReviewRenderKey,
    pub next_states: ReviewSchedulingStates,
    pub predicted_next: ReviewPredictions,
    pub flag: u8,
    pub marked: bool,
    pub home_preset: ReviewHomePreset,
}

impl From<session::HeadItem> for ReviewHeadItem {
    fn from(v: session::HeadItem) -> Self {
        Self {
            card_id: v.card_id.0,
            note_id: v.note_id.0,
            kind: v.kind.into(),
            state_token: ReviewStateToken {
                bytes: v.state_token.to_vec(),
            },
            render_key: ReviewRenderKey {
                bytes: v.render_key.to_vec(),
            },
            next_states: v.next_states.into(),
            predicted_next: ReviewPredictions {
                again: v.predicted_next[0].map(|id| id.0),
                hard: v.predicted_next[1].map(|id| id.0),
                good: v.predicted_next[2].map(|id| id.0),
                easy: v.predicted_next[3].map(|id| id.0),
            },
            flag: v.flag,
            marked: v.marked,
            home_preset: v.home_preset.into(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct ReviewCounts {
    pub new: u32,
    pub learning: u32,
    pub review: u32,
}

impl From<sched::queue::Counts> for ReviewCounts {
    fn from(v: sched::queue::Counts) -> Self {
        Self {
            new: v.new,
            learning: v.learning,
            review: v.review,
        }
    }
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct ReviewHead {
    pub items: Vec<ReviewHeadItem>,
    pub counts: ReviewCounts,
    pub valid_until: i64,
}

impl From<session::Head> for ReviewHead {
    fn from(v: session::Head) -> Self {
        Self {
            items: v.items.into_iter().map(Into::into).collect(),
            counts: v.counts.into(),
            valid_until: v.valid_until,
        }
    }
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct ReviewMarks {
    pub added: bool,
    pub restate: bool,
    pub rerender: bool,
    pub relabel: bool,
}

impl From<session::Marks> for ReviewMarks {
    fn from(v: session::Marks) -> Self {
        Self {
            added: v.added,
            restate: v.restate,
            rerender: v.rerender,
            relabel: v.relabel,
        }
    }
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct ReviewHeadDiff {
    pub head: ReviewHead,
    pub marks: Vec<ReviewMarks>,
    pub removed: Vec<i64>,
}

impl From<session::HeadDiff> for ReviewHeadDiff {
    fn from(v: session::HeadDiff) -> Self {
        Self {
            head: v.head.into(),
            marks: v.marks.into_iter().map(Into::into).collect(),
            removed: v.removed.into_iter().map(|id| id.0).collect(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct ReviewDifference {
    pub new: i64,
    pub learning: i64,
    pub review: i64,
}

impl From<sched::counts::finished::Difference> for ReviewDifference {
    fn from(v: sched::counts::finished::Difference) -> Self {
        Self {
            new: v.new,
            learning: v.learning,
            review: v.review,
        }
    }
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct ReviewDeckStudyState {
    pub deck: i64,
    pub finished: bool,
    pub empty: bool,
    pub queue_counts: ReviewCounts,
    pub list_counts: ReviewCounts,
    pub difference: ReviewDifference,
    pub tomorrow: ReviewCounts,
    pub new_limit_reached: bool,
    pub new_remaining: bool,
    pub review_limit_reached: bool,
    pub review_remaining: bool,
    pub learning_remaining: u32,
    pub learning_remaining_today: u32,
    pub learning_later_today: u32,
    pub secs_until_next_learn: i64,
    pub next_learning_due: Option<i64>,
    pub next_learning_ready_at: Option<i64>,
    pub sibling_buried: u32,
    pub user_buried: u32,
}

impl From<sched::counts::finished::DeckStudyState> for ReviewDeckStudyState {
    fn from(v: sched::counts::finished::DeckStudyState) -> Self {
        Self {
            deck: v.deck.0,
            finished: v.finished,
            empty: v.empty,
            queue_counts: v.queue_counts.into(),
            list_counts: v.list_counts.into(),
            difference: v.difference.into(),
            tomorrow: v.tomorrow.into(),
            new_limit_reached: v.new_limit_reached,
            new_remaining: v.new_remaining,
            review_limit_reached: v.review_limit_reached,
            review_remaining: v.review_remaining,
            learning_remaining: v.learning_remaining,
            learning_remaining_today: v.learning_remaining_today,
            learning_later_today: v.learning_later_today,
            secs_until_next_learn: v.secs_until_next_learn,
            next_learning_due: v.next_learning_due,
            next_learning_ready_at: v.next_learning_ready_at,
            sibling_buried: v.sibling_buried,
            user_buried: v.user_buried,
        }
    }
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct ReviewDuration {
    pub seconds: u64,
    pub nanoseconds: u32,
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct ReviewStepInfo {
    pub id: u64,
    pub name: String,
    pub kind: ReviewStepKind,
    pub session: Option<u64>,
    pub long_write: Option<ReviewDuration>,
}

impl From<engine::StepInfo> for ReviewStepInfo {
    fn from(v: engine::StepInfo) -> Self {
        Self {
            id: v.id.0,
            name: v.name.to_owned(),
            kind: v.kind.into(),
            session: v.session.map(|id| id.0),
            long_write: v.long_write.map(|d| ReviewDuration {
                seconds: d.as_secs(),
                nanoseconds: d.subsec_nanos(),
            }),
        }
    }
}

#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum ReviewSchedulingState {
    New {
        position: u32,
    },
    Learning {
        state: ReviewLearningState,
    },
    Review {
        state: ReviewReviewState,
    },
    Relearning {
        learning: ReviewLearningState,
        review: ReviewReviewState,
    },
}

#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum ReviewDelay {
    Seconds { amount: u32 },
    Days { amount: u32 },
}

#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum ReviewTimeUnit {
    Seconds,
    Minutes,
    Hours,
    Days,
    Months,
    Years,
    End,
}

#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum ReviewQueueKind {
    New,
    Review,
    InterdayLearning,
    Learning,
}

#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum ReviewRating {
    Again,
    Hard,
    Good,
    Easy,
}

#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum ReviewRefusal {
    FsrsPolicyPending,
    PresetUnsupported { preset_ids: Vec<i64> },
}

#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum ReviewStartResult {
    Started {
        session_id: u64,
        generation: u64,
        head: ReviewHead,
        time_lim: u32,
    },
    Refused {
        reason: ReviewRefusal,
    },
}

#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum ReviewPeekResult {
    Card { item: ReviewHeadItem },
    Empty,
    Refused { reason: ReviewRefusal },
}

#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum ReviewAnswerOutcome {
    Accepted {
        head_diff: ReviewHeadDiff,
        leech: bool,
        suspended: bool,
        rolled_over: bool,
    },
    Stale {
        fresh_head: ReviewHead,
        notice: bool,
    },
    NotAtHead {
        fresh_head: ReviewHead,
    },
}

#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum ReviewCurrentChange {
    Eligible,
    Removed,
    MemoryOnly,
}

#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum ReviewUndoScope {
    Global,
    Session { session_id: u64 },
}

#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum ReviewDisabledReason {
    None,
    ClearedByRollover,
    ClearedBySync,
    ClearedByOneWay,
    ClearedByNonUndoableOp,
    ChangedExternally,
    AlreadyUploaded,
    UploadPending,
}

#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum ReviewStepKind {
    Answer,
    Other,
}

#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum ReviewUndoStatus {
    Available { step: ReviewStepInfo },
    Unavailable { reason: ReviewDisabledReason },
}

impl From<sched::states::labels::TimeUnit> for ReviewTimeUnit {
    fn from(v: sched::states::labels::TimeUnit) -> Self {
        match v {
            sched::states::labels::TimeUnit::Seconds => Self::Seconds,
            sched::states::labels::TimeUnit::Minutes => Self::Minutes,
            sched::states::labels::TimeUnit::Hours => Self::Hours,
            sched::states::labels::TimeUnit::Days => Self::Days,
            sched::states::labels::TimeUnit::Months => Self::Months,
            sched::states::labels::TimeUnit::Years => Self::Years,
            sched::states::labels::TimeUnit::End => Self::End,
        }
    }
}

impl From<sched::queue::Kind> for ReviewQueueKind {
    fn from(v: sched::queue::Kind) -> Self {
        match v {
            sched::queue::Kind::New => Self::New,
            sched::queue::Kind::Review => Self::Review,
            sched::queue::Kind::InterdayLearning => Self::InterdayLearning,
            sched::queue::Kind::Learning => Self::Learning,
        }
    }
}

impl From<session::CurrentChange> for ReviewCurrentChange {
    fn from(v: session::CurrentChange) -> Self {
        match v {
            session::CurrentChange::Eligible => Self::Eligible,
            session::CurrentChange::Removed => Self::Removed,
            session::CurrentChange::MemoryOnly => Self::MemoryOnly,
        }
    }
}

impl From<engine::DisabledReason> for ReviewDisabledReason {
    fn from(v: engine::DisabledReason) -> Self {
        match v {
            engine::DisabledReason::None => Self::None,
            engine::DisabledReason::ClearedByRollover => Self::ClearedByRollover,
            engine::DisabledReason::ClearedBySync => Self::ClearedBySync,
            engine::DisabledReason::ClearedByOneWay => Self::ClearedByOneWay,
            engine::DisabledReason::ClearedByNonUndoableOp => Self::ClearedByNonUndoableOp,
            engine::DisabledReason::ChangedExternally => Self::ChangedExternally,
            engine::DisabledReason::AlreadyUploaded => Self::AlreadyUploaded,
            engine::DisabledReason::UploadPending => Self::UploadPending,
        }
    }
}

impl From<engine::StepKind> for ReviewStepKind {
    fn from(v: engine::StepKind) -> Self {
        match v {
            engine::StepKind::Answer => Self::Answer,
            engine::StepKind::Other => Self::Other,
        }
    }
}

impl From<states::State> for ReviewSchedulingState {
    fn from(v: states::State) -> Self {
        match v {
            states::State::New { position } => Self::New { position },
            states::State::Learning(v) => Self::Learning { state: v.into() },
            states::State::Review(v) => Self::Review { state: v.into() },
            states::State::Relearning { learning, review } => Self::Relearning {
                learning: learning.into(),
                review: review.into(),
            },
        }
    }
}
impl From<sched::states::interval_limits::Delay> for ReviewDelay {
    fn from(v: sched::states::interval_limits::Delay) -> Self {
        match v {
            sched::states::interval_limits::Delay::Seconds(amount) => Self::Seconds { amount },
            sched::states::interval_limits::Delay::Days(amount) => Self::Days { amount },
        }
    }
}
impl From<ReviewRating> for sched::answer::Rating {
    fn from(v: ReviewRating) -> Self {
        match v {
            ReviewRating::Again => Self::Again,
            ReviewRating::Hard => Self::Hard,
            ReviewRating::Good => Self::Good,
            ReviewRating::Easy => Self::Easy,
        }
    }
}
impl From<ReviewUndoScope> for engine::UndoScope {
    fn from(v: ReviewUndoScope) -> Self {
        match v {
            ReviewUndoScope::Global => Self::Global,
            ReviewUndoScope::Session { session_id } => Self::Session(engine::SessionId(session_id)),
        }
    }
}
impl From<engine::UndoStatus> for ReviewUndoStatus {
    fn from(v: engine::UndoStatus) -> Self {
        match v {
            engine::UndoStatus::Available(step) => Self::Available { step: step.into() },
            engine::UndoStatus::Unavailable(reason) => Self::Unavailable { reason: reason.into() },
        }
    }
}
impl From<session::Refusal> for ReviewRefusal {
    fn from(v: session::Refusal) -> Self {
        match v {
            session::Refusal::FsrsPolicyPending => Self::FsrsPolicyPending,
            session::Refusal::PresetUnsupported { preset_ids } => Self::PresetUnsupported {
                preset_ids: preset_ids.into_iter().map(|id| id.0).collect(),
            },
        }
    }
}
impl From<session::StartResult> for ReviewStartResult {
    fn from(v: session::StartResult) -> Self {
        match v {
            session::StartResult::Started {
                session_id,
                generation,
                head,
                time_lim,
            } => Self::Started {
                session_id: session_id.0,
                generation: generation.0,
                head: head.into(),
                time_lim,
            },
            session::StartResult::Refused(reason) => Self::Refused { reason: reason.into() },
        }
    }
}
impl From<session::PeekResult> for ReviewPeekResult {
    fn from(v: session::PeekResult) -> Self {
        match v {
            session::PeekResult::Card(item) => Self::Card { item: item.into() },
            session::PeekResult::Empty => Self::Empty,
            session::PeekResult::Refused(reason) => Self::Refused { reason: reason.into() },
        }
    }
}
impl From<session::AnswerOutcome> for ReviewAnswerOutcome {
    fn from(v: session::AnswerOutcome) -> Self {
        match v {
            session::AnswerOutcome::Accepted {
                head_diff,
                leech,
                suspended,
                rolled_over,
            } => Self::Accepted {
                head_diff: head_diff.into(),
                leech,
                suspended,
                rolled_over,
            },
            session::AnswerOutcome::Stale { fresh_head, notice } => Self::Stale {
                fresh_head: fresh_head.into(),
                notice,
            },
            session::AnswerOutcome::NotAtHead { fresh_head } => Self::NotAtHead {
                fresh_head: fresh_head.into(),
            },
        }
    }
}
fn digest(bytes: Vec<u8>, name: &str) -> Result<[u8; 32], CoreError> {
    bytes.try_into().map_err(|_| CoreError::InvalidArgument {
        message: format!("{name} must contain exactly 32 bytes"),
    })
}
#[uniffi::export]
pub fn start_session(deck_id: i64, resume_card_id: Option<i64>) -> Result<ReviewStartResult, CoreError> {
    callback::enter("start_session");
    Ok(runtime::review()?
        .start(DeckId(deck_id), resume_card_id.map(CardId))?
        .into())
}
#[uniffi::export]
pub fn peek_first_card(deck_id: i64) -> Result<ReviewPeekResult, CoreError> {
    callback::enter("peek_first_card");
    Ok(runtime::review()?.peek_first_card(DeckId(deck_id))?.into())
}
#[uniffi::export]
pub fn session_head(session_id: u64, count: u32) -> Result<ReviewHead, CoreError> {
    callback::enter("session_head");
    let count = usize::try_from(count).map_err(|_| CoreError::InvalidArgument {
        message: "head count exceeds native size".into(),
    })?;
    Ok(runtime::review()?.head(engine::SessionId(session_id), count)?.into())
}
#[uniffi::export]
pub fn refresh_session_card(session_id: u64, card_id: i64) -> Result<ReviewHead, CoreError> {
    callback::enter("refresh_session_card");
    Ok(runtime::review()?
        .refresh_card(engine::SessionId(session_id), CardId(card_id))?
        .into())
}
#[uniffi::export]
pub fn render_session_head(
    session_id: u64,
    keys: Vec<ReviewRenderKey>,
    platform: RenderPlatform,
) -> Result<Vec<RenderedCard>, CoreError> {
    callback::enter("render_session_head");
    let keys = keys
        .into_iter()
        .map(|key| digest(key.bytes, "render key"))
        .collect::<Result<Vec<_>, _>>()?;
    let platform = match platform {
        RenderPlatform::Mac => engine::rendering::Platform::Mac,
        RenderPlatform::Ios => engine::rendering::Platform::Ios,
    };
    Ok(runtime::review()?
        .render_head(engine::SessionId(session_id), &keys, platform)?
        .into_iter()
        .map(|card| card.as_ref().clone().into())
        .collect())
}
#[uniffi::export]
pub fn answer_session(
    session_id: u64,
    card_id: i64,
    rating: ReviewRating,
    state_token: ReviewStateToken,
    answered_at_ms: i64,
    elapsed_ms: u64,
) -> Result<ReviewAnswerOutcome, CoreError> {
    callback::enter("answer_session");
    let token = digest(state_token.bytes, "state token")?;
    Ok(runtime::review()?
        .answer(
            engine::SessionId(session_id),
            CardId(card_id),
            rating.into(),
            token,
            EpochMillis(answered_at_ms),
            elapsed_ms,
        )?
        .into())
}
#[uniffi::export]
pub fn end_session(session_id: u64) -> Result<(), CoreError> {
    callback::enter("end_session");
    Ok(runtime::review()?.end(engine::SessionId(session_id))?)
}
#[uniffi::export]
pub fn peek_review_undo(scope: ReviewUndoScope) -> Result<ReviewUndoStatus, CoreError> {
    callback::enter("peek_review_undo");
    Ok(runtime::review()?.peek_undo(scope.into())?.into())
}
#[uniffi::export]
pub fn flush_review_for_background() -> Result<(), CoreError> {
    callback::enter("flush_review_for_background");
    Ok(runtime::review()?.flush_for_background()?)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use meguri_model::ids::{DeckConfigId, NoteId};

    pub(crate) fn sample_head() -> session::Head {
        let memory = Some(meguri_model::card_data::MemoryState {
            stability: 2.75,
            difficulty: 6.125,
        });
        let learning = sched::states::learning::LearningState {
            remaining_steps: 3,
            scheduled_secs: 91,
            elapsed_secs: 17,
            memory_state: memory,
        };
        let review = states::ReviewState {
            scheduled_days: 24,
            elapsed_days: 29,
            ease_factor: 2.375,
            lapses: 8,
            leeched: true,
            memory_state: memory,
        };
        let answer = |state, delay, amount: &str, unit| states::AnswerState {
            state,
            delay,
            left: 3,
            factor: 2375,
            revlog: states::RevlogPreview {
                interval: -91,
                last_interval: 24,
                kind: 2,
                factor: 750,
            },
            label: sched::states::labels::AnswerLabel {
                less_than: true,
                amount: amount.into(),
                unit,
            },
        };
        session::Head {
            items: vec![session::HeadItem {
                card_id: CardId(-41),
                note_id: NoteId(-51),
                kind: sched::queue::Kind::InterdayLearning,
                state_token: [0x17; 32],
                render_key: [0xa3; 32],
                next_states: states::SchedulingStates {
                    current: states::State::New { position: 97 },
                    again: answer(
                        states::State::Learning(learning),
                        sched::states::interval_limits::Delay::Seconds(91),
                        "2",
                        sched::states::labels::TimeUnit::Minutes,
                    ),
                    hard: answer(
                        states::State::Review(review),
                        sched::states::interval_limits::Delay::Days(24),
                        "24",
                        sched::states::labels::TimeUnit::Days,
                    ),
                    good: answer(
                        states::State::Relearning { learning, review },
                        sched::states::interval_limits::Delay::Seconds(121),
                        "3",
                        sched::states::labels::TimeUnit::Minutes,
                    ),
                    easy: answer(
                        states::State::New { position: 12 },
                        sched::states::interval_limits::Delay::Days(365),
                        "1",
                        sched::states::labels::TimeUnit::Years,
                    ),
                },
                predicted_next: [Some(CardId(-42)), None, Some(CardId(43)), Some(CardId(44))],
                flag: 7,
                marked: true,
                home_preset: session::HomePreset {
                    id: DeckConfigId(-61),
                    autoplay: true,
                    skip_question_when_replaying_answer: true,
                    show_timer: true,
                    stop_timer_on_answer: false,
                    maximum_answer_seconds: 123,
                    question_seconds: 1.25,
                    answer_seconds: 2.75,
                    question_action: 2,
                    answer_action: 3,
                    wait_for_audio: true,
                    leech_action: 1,
                },
            }],
            counts: sched::queue::Counts {
                new: 11,
                learning: 12,
                review: 13,
            },
            valid_until: 1_894_701_723,
        }
    }
    pub(crate) fn sample_diff() -> session::HeadDiff {
        session::HeadDiff {
            head: sample_head(),
            marks: vec![session::Marks {
                added: true,
                restate: false,
                rerender: true,
                relabel: true,
            }],
            removed: vec![CardId(-70), CardId(71)],
        }
    }
    pub(crate) fn sample_finished() -> sched::counts::finished::DeckStudyState {
        sched::counts::finished::DeckStudyState {
            deck: DeckId(-4),
            queue_counts: sched::queue::Counts {
                new: 1,
                learning: 2,
                review: 3,
            },
            list_counts: sched::queue::Counts {
                new: 4,
                learning: 5,
                review: 6,
            },
            difference: sched::counts::finished::Difference {
                new: -3,
                learning: 3,
                review: -9,
            },
            finished: true,
            learning_remaining_today: 14,
            learning_later_today: 15,
            next_learning_due: Some(-123),
            next_learning_ready_at: Some(-456),
            review_limit_reached: true,
            new_limit_reached: false,
            learning_remaining: 16,
            secs_until_next_learn: -17,
            review_remaining: false,
            new_remaining: true,
            sibling_buried: 18,
            user_buried: 19,
            tomorrow: sched::queue::Counts {
                new: 7,
                learning: 8,
                review: 9,
            },
            empty: false,
        }
    }
    #[test]
    fn complete_head_preserves_states_predictions_tokens_preset_and_units() {
        let head: ReviewHead = sample_head().into();
        assert_eq!(
            head.counts,
            ReviewCounts {
                new: 11,
                learning: 12,
                review: 13
            }
        );
        assert_eq!(head.valid_until, 1_894_701_723);
        let item = &head.items[0];
        assert_eq!(
            (item.card_id, item.note_id, item.flag, item.marked),
            (-41, -51, 7, true)
        );
        assert_eq!(item.kind, ReviewQueueKind::InterdayLearning);
        assert_eq!(item.state_token.bytes, [0x17; 32]);
        assert_eq!(item.render_key.bytes, [0xa3; 32]);
        assert_eq!(
            item.predicted_next,
            ReviewPredictions {
                again: Some(-42),
                hard: None,
                good: Some(43),
                easy: Some(44)
            }
        );
        assert_eq!(item.next_states.current, ReviewSchedulingState::New { position: 97 });
        let ReviewSchedulingState::Learning { state: learning } = &item.next_states.again.state else {
            panic!("learning")
        };
        assert_eq!(
            (learning.remaining_steps, learning.scheduled_secs, learning.elapsed_secs),
            (3, 91, 17)
        );
        assert_eq!(
            learning.memory_state,
            Some(ReviewMemoryState {
                stability: 2.75,
                difficulty: 6.125
            })
        );
        let ReviewSchedulingState::Review { state: review } = &item.next_states.hard.state else {
            panic!("review")
        };
        assert_eq!(
            (
                review.scheduled_days,
                review.elapsed_days,
                review.ease_factor,
                review.lapses,
                review.leeched
            ),
            (24, 29, 2.375, 8, true)
        );
        assert_eq!(review.memory_state, learning.memory_state);
        assert_eq!(
            item.next_states.good.state,
            ReviewSchedulingState::Relearning {
                learning: learning.clone(),
                review: review.clone()
            }
        );
        assert_eq!(item.next_states.easy.state, ReviewSchedulingState::New { position: 12 });
        assert_eq!(item.next_states.again.delay, ReviewDelay::Seconds { amount: 91 });
        assert_eq!(item.next_states.hard.delay, ReviewDelay::Days { amount: 24 });
        assert_eq!((item.next_states.good.left, item.next_states.good.factor), (3, 2375));
        assert_eq!(
            item.next_states.good.revlog,
            ReviewRevlogPreview {
                interval: -91,
                last_interval: 24,
                kind: 2,
                factor: 750
            }
        );
        assert_eq!(
            item.next_states.easy.label,
            ReviewAnswerLabel {
                less_than: true,
                amount: "1".into(),
                unit: ReviewTimeUnit::Years
            }
        );
        assert_eq!(
            item.home_preset,
            ReviewHomePreset {
                id: -61,
                autoplay: true,
                skip_question_when_replaying_answer: true,
                show_timer: true,
                stop_timer_on_answer: false,
                maximum_answer_seconds: 123,
                question_seconds: 1.25,
                answer_seconds: 2.75,
                question_action: 2,
                answer_action: 3,
                wait_for_audio: true,
                leech_action: 1,
            }
        );
        for (source, expected) in [
            (sched::queue::Kind::New, ReviewQueueKind::New),
            (sched::queue::Kind::Review, ReviewQueueKind::Review),
            (sched::queue::Kind::Learning, ReviewQueueKind::Learning),
            (sched::queue::Kind::InterdayLearning, ReviewQueueKind::InterdayLearning),
        ] {
            assert_eq!(ReviewQueueKind::from(source), expected);
        }
        for (source, expected) in [
            (sched::states::labels::TimeUnit::Seconds, ReviewTimeUnit::Seconds),
            (sched::states::labels::TimeUnit::Minutes, ReviewTimeUnit::Minutes),
            (sched::states::labels::TimeUnit::Hours, ReviewTimeUnit::Hours),
            (sched::states::labels::TimeUnit::Days, ReviewTimeUnit::Days),
            (sched::states::labels::TimeUnit::Months, ReviewTimeUnit::Months),
            (sched::states::labels::TimeUnit::Years, ReviewTimeUnit::Years),
            (sched::states::labels::TimeUnit::End, ReviewTimeUnit::End),
        ] {
            assert_eq!(ReviewTimeUnit::from(source), expected);
        }
    }
    #[test]
    fn admission_answer_and_diff_results_keep_refusals_notices_and_marks() {
        let head: ReviewHead = sample_head().into();
        assert_eq!(
            ReviewStartResult::from(session::StartResult::Started {
                session_id: engine::SessionId(u64::MAX),
                generation: engine::Generation(91),
                head: sample_head(),
                time_lim: 123,
            }),
            ReviewStartResult::Started {
                session_id: u64::MAX,
                generation: 91,
                head: head.clone(),
                time_lim: 123
            }
        );
        for (reason, expected) in [
            (session::Refusal::FsrsPolicyPending, ReviewRefusal::FsrsPolicyPending),
            (
                session::Refusal::PresetUnsupported {
                    preset_ids: vec![DeckConfigId(-2), DeckConfigId(9)],
                },
                ReviewRefusal::PresetUnsupported {
                    preset_ids: vec![-2, 9],
                },
            ),
        ] {
            assert_eq!(
                ReviewStartResult::from(session::StartResult::Refused(reason.clone())),
                ReviewStartResult::Refused {
                    reason: expected.clone()
                }
            );
            assert_eq!(
                ReviewPeekResult::from(session::PeekResult::Refused(reason)),
                ReviewPeekResult::Refused { reason: expected }
            );
        }
        assert_eq!(
            ReviewPeekResult::from(session::PeekResult::Empty),
            ReviewPeekResult::Empty
        );
        assert_eq!(
            ReviewPeekResult::from(session::PeekResult::Card(sample_head().items.remove(0))),
            ReviewPeekResult::Card {
                item: head.items[0].clone()
            }
        );
        let diff: ReviewHeadDiff = sample_diff().into();
        assert_eq!(diff.head, head);
        assert_eq!(
            diff.marks,
            [ReviewMarks {
                added: true,
                restate: false,
                rerender: true,
                relabel: true
            }]
        );
        assert_eq!(diff.removed, [-70, 71]);
        assert_eq!(
            ReviewAnswerOutcome::from(session::AnswerOutcome::Accepted {
                head_diff: sample_diff(),
                leech: true,
                suspended: false,
                rolled_over: true
            }),
            ReviewAnswerOutcome::Accepted {
                head_diff: diff,
                leech: true,
                suspended: false,
                rolled_over: true
            }
        );
        for notice in [true, false] {
            assert_eq!(
                ReviewAnswerOutcome::from(session::AnswerOutcome::Stale {
                    fresh_head: sample_head(),
                    notice
                }),
                ReviewAnswerOutcome::Stale {
                    fresh_head: head.clone(),
                    notice
                }
            );
        }
        assert_eq!(
            ReviewAnswerOutcome::from(session::AnswerOutcome::NotAtHead {
                fresh_head: sample_head()
            }),
            ReviewAnswerOutcome::NotAtHead { fresh_head: head }
        );
    }
    #[test]
    fn undo_scopes_steps_all_disabled_reasons_and_duration_keep_typed_values() {
        assert_eq!(
            engine::UndoScope::from(ReviewUndoScope::Global),
            engine::UndoScope::Global
        );
        assert_eq!(
            engine::UndoScope::from(ReviewUndoScope::Session { session_id: u64::MAX }),
            engine::UndoScope::Session(engine::SessionId(u64::MAX))
        );
        for (source, expected) in [
            (engine::DisabledReason::None, ReviewDisabledReason::None),
            (
                engine::DisabledReason::ClearedByRollover,
                ReviewDisabledReason::ClearedByRollover,
            ),
            (
                engine::DisabledReason::ClearedBySync,
                ReviewDisabledReason::ClearedBySync,
            ),
            (
                engine::DisabledReason::ClearedByOneWay,
                ReviewDisabledReason::ClearedByOneWay,
            ),
            (
                engine::DisabledReason::ClearedByNonUndoableOp,
                ReviewDisabledReason::ClearedByNonUndoableOp,
            ),
            (
                engine::DisabledReason::ChangedExternally,
                ReviewDisabledReason::ChangedExternally,
            ),
            (
                engine::DisabledReason::AlreadyUploaded,
                ReviewDisabledReason::AlreadyUploaded,
            ),
            (
                engine::DisabledReason::UploadPending,
                ReviewDisabledReason::UploadPending,
            ),
        ] {
            assert_eq!(
                ReviewUndoStatus::from(engine::UndoStatus::Unavailable(source)),
                ReviewUndoStatus::Unavailable { reason: expected }
            );
        }
        assert_eq!(
            ReviewUndoStatus::from(engine::UndoStatus::Available(engine::StepInfo {
                id: engine::EntryId(u64::MAX),
                name: "Review bridge step",
                kind: engine::StepKind::Answer,
                session: Some(engine::SessionId(91)),
                long_write: Some(std::time::Duration::new(3, 123_456_789)),
            })),
            ReviewUndoStatus::Available {
                step: ReviewStepInfo {
                    id: u64::MAX,
                    name: "Review bridge step".into(),
                    kind: ReviewStepKind::Answer,
                    session: Some(91),
                    long_write: Some(ReviewDuration {
                        seconds: 3,
                        nanoseconds: 123_456_789
                    }),
                }
            }
        );
        assert_eq!(ReviewStepKind::from(engine::StepKind::Other), ReviewStepKind::Other);
        for (rating, expected) in [
            (ReviewRating::Again, sched::answer::Rating::Again),
            (ReviewRating::Hard, sched::answer::Rating::Hard),
            (ReviewRating::Good, sched::answer::Rating::Good),
            (ReviewRating::Easy, sched::answer::Rating::Easy),
        ] {
            assert_eq!(sched::answer::Rating::from(rating), expected);
        }
    }
    #[test]
    fn malformed_token_and_render_key_inputs_are_rejected_before_service_entry() {
        for size in [0, 1, 31, 33, 64] {
            assert!(matches!(
                answer_session(
                    1,
                    2,
                    ReviewRating::Good,
                    ReviewStateToken { bytes: vec![0; size] },
                    i64::MAX,
                    u64::MAX
                ),
                Err(CoreError::InvalidArgument { .. })
            ));
            assert!(matches!(
                render_session_head(1, vec![ReviewRenderKey { bytes: vec![0; size] }], RenderPlatform::Mac),
                Err(CoreError::InvalidArgument { .. })
            ));
        }
        assert_eq!(digest(vec![77; 32], "state token").unwrap(), [77; 32]);
    }
}
