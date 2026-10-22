//! Deck-options editor values, metadata and patch-only codecs (O2–O5).
use meguri_model::proto::deck_config::DeckConfigMessage;
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Field {
    NewPerDay,
    ReviewsPerDay,
    DesiredRetention,
    MinimumLapseInterval,
    LeechThreshold,
    CapAnswerTimeToSecs,
    SecondsToShowQuestion,
    SecondsToShowAnswer,
    MaximumReviewInterval,
    HistoricalRetention,
    NewCardInsertOrder,
    LeechAction,
    NewCardGatherPriority,
    NewCardSortOrder,
    NewMix,
    InterdayLearningMix,
    ReviewOrder,
    QuestionAction,
    AnswerAction,
    BuryNew,
    BuryReviews,
    BuryInterdayLearning,
    DisableAutoplay,
    SkipQuestionWhenReplayingAnswer,
    ShowTimer,
    StopTimerOnAnswer,
    WaitForAudio,
    LearnSteps,
    RelearnSteps,
    IgnoreRevlogsBeforeDate,
    NewLimit,
    ReviewLimit,
    NewLimitToday,
    ReviewLimitToday,
    DeckDesiredRetention,
    NewCardsIgnoreReviewLimit,
    ApplyAllParentLimits,
    LoadBalancerEnabled,
    Rollover,
    LearnAheadMinutes,
}
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Number(f64),
    Boolean(bool),
    Text(String),
    Steps(Vec<f32>),
    None,
}
impl Value {
    pub fn same(&self, other: &Self, field: Field) -> bool {
        match (self, other) {
            (Self::Number(a), Self::Number(b)) if field.is_float() => {
                (*a as f32 == *b as f32) || (a.to_bits() == b.to_bits())
            }
            (Self::Steps(a), Self::Steps(b)) => {
                a.len() == b.len() && a.iter().zip(b).all(|(a, b)| a == b || a.to_bits() == b.to_bits())
            }
            _ => self == other,
        }
    }
}
#[derive(Clone, Debug, PartialEq)]
pub struct Metadata {
    pub field: Field,
    pub default: Value,
    pub minimum: Option<f64>,
    pub maximum: Option<f64>,
    pub step: Option<f64>,
    pub whole: bool,
    pub choices: Vec<i32>,
    pub global: bool,
    pub optional: bool,
}
impl Field {
    pub const PRESET: &'static [Self] = &[
        Self::NewPerDay,
        Self::ReviewsPerDay,
        Self::DesiredRetention,
        Self::MinimumLapseInterval,
        Self::LeechThreshold,
        Self::CapAnswerTimeToSecs,
        Self::SecondsToShowQuestion,
        Self::SecondsToShowAnswer,
        Self::MaximumReviewInterval,
        Self::HistoricalRetention,
        Self::NewCardInsertOrder,
        Self::LeechAction,
        Self::NewCardGatherPriority,
        Self::NewCardSortOrder,
        Self::NewMix,
        Self::InterdayLearningMix,
        Self::ReviewOrder,
        Self::QuestionAction,
        Self::AnswerAction,
        Self::BuryNew,
        Self::BuryReviews,
        Self::BuryInterdayLearning,
        Self::DisableAutoplay,
        Self::SkipQuestionWhenReplayingAnswer,
        Self::ShowTimer,
        Self::StopTimerOnAnswer,
        Self::WaitForAudio,
        Self::LearnSteps,
        Self::RelearnSteps,
        Self::IgnoreRevlogsBeforeDate,
    ];
    pub const DECK: &'static [Self] = &[
        Self::NewLimit,
        Self::ReviewLimit,
        Self::NewLimitToday,
        Self::ReviewLimitToday,
        Self::DeckDesiredRetention,
    ];
    pub const GLOBAL: &'static [Self] = &[
        Self::NewCardsIgnoreReviewLimit,
        Self::ApplyAllParentLimits,
        Self::LoadBalancerEnabled,
    ];
    pub fn is_float(self) -> bool {
        matches!(
            self,
            Self::DesiredRetention
                | Self::SecondsToShowQuestion
                | Self::SecondsToShowAnswer
                | Self::HistoricalRetention
                | Self::DeckDesiredRetention
        )
    }
    pub fn name(self) -> &'static str {
        match self {
            Self::NewPerDay => "new_per_day",
            Self::ReviewsPerDay => "reviews_per_day",
            Self::DesiredRetention => "desired_retention",
            Self::MinimumLapseInterval => "minimum_lapse_interval",
            Self::LeechThreshold => "leech_threshold",
            Self::CapAnswerTimeToSecs => "cap_answer_time_to_secs",
            Self::SecondsToShowQuestion => "seconds_to_show_question",
            Self::SecondsToShowAnswer => "seconds_to_show_answer",
            Self::MaximumReviewInterval => "maximum_review_interval",
            Self::HistoricalRetention => "historical_retention",
            Self::NewCardInsertOrder => "new_card_insert_order",
            Self::LeechAction => "leech_action",
            Self::NewCardGatherPriority => "new_card_gather_priority",
            Self::NewCardSortOrder => "new_card_sort_order",
            Self::NewMix => "new_mix",
            Self::InterdayLearningMix => "interday_learning_mix",
            Self::ReviewOrder => "review_order",
            Self::QuestionAction => "question_action",
            Self::AnswerAction => "answer_action",
            Self::BuryNew => "bury_new",
            Self::BuryReviews => "bury_reviews",
            Self::BuryInterdayLearning => "bury_interday_learning",
            Self::DisableAutoplay => "disable_autoplay",
            Self::SkipQuestionWhenReplayingAnswer => "skip_question_when_replaying_answer",
            Self::ShowTimer => "show_timer",
            Self::StopTimerOnAnswer => "stop_timer_on_answer",
            Self::WaitForAudio => "wait_for_audio",
            Self::LearnSteps => "learn_steps",
            Self::RelearnSteps => "relearn_steps",
            Self::IgnoreRevlogsBeforeDate => "ignore_revlogs_before_date",
            Self::NewLimit => "new_limit",
            Self::ReviewLimit => "review_limit",
            Self::NewLimitToday => "new_limit_today",
            Self::ReviewLimitToday => "review_limit_today",
            Self::DeckDesiredRetention => "deck_desired_retention",
            Self::NewCardsIgnoreReviewLimit => "new_cards_ignore_review_limit",
            Self::ApplyAllParentLimits => "apply_all_parent_limits",
            Self::LoadBalancerEnabled => "load_balancer_enabled",
            Self::Rollover => "rollover",
            Self::LearnAheadMinutes => "learn_ahead_minutes",
        }
    }
    pub fn metadata(self) -> Metadata {
        let mut m = Metadata {
            field: self,
            default: Value::None,
            minimum: None,
            maximum: None,
            step: None,
            whole: false,
            choices: vec![],
            global: Self::GLOBAL.contains(&self),
            optional: Self::DECK.contains(&self),
        };
        match self {
            Self::NewPerDay => {
                m.default = Value::Number(20.0);
                m.minimum = Some(0.0);
                m.maximum = Some(9999.0);
                m.whole = true;
            }
            Self::ReviewsPerDay => {
                m.default = Value::Number(200.0);
                m.minimum = Some(0.0);
                m.maximum = Some(9999.0);
                m.whole = true;
            }
            Self::DesiredRetention => {
                m.default = Value::Number(0.9);
                m.minimum = Some(0.7);
                m.maximum = Some(0.99);
                m.whole = false;
            }
            Self::MinimumLapseInterval => {
                m.default = Value::Number(1.0);
                m.minimum = Some(1.0);
                m.maximum = Some(9999.0);
                m.whole = true;
            }
            Self::LeechThreshold => {
                m.default = Value::Number(8.0);
                m.minimum = Some(1.0);
                m.maximum = Some(9999.0);
                m.whole = true;
            }
            Self::CapAnswerTimeToSecs => {
                m.default = Value::Number(60.0);
                m.minimum = Some(1.0);
                m.maximum = Some(7200.0);
                m.whole = true;
            }
            Self::SecondsToShowQuestion => {
                m.default = Value::Number(0.0);
                m.minimum = Some(0.0);
                m.maximum = Some(9999.0);
                m.whole = false;
            }
            Self::SecondsToShowAnswer => {
                m.default = Value::Number(0.0);
                m.minimum = Some(0.0);
                m.maximum = Some(9999.0);
                m.whole = false;
            }
            Self::MaximumReviewInterval => {
                m.default = Value::Number(36500.0);
                m.minimum = Some(1.0);
                m.maximum = Some(36500.0);
                m.whole = true;
            }
            Self::HistoricalRetention => {
                m.default = Value::Number(0.9);
                m.minimum = Some(0.7);
                m.maximum = Some(0.97);
                m.whole = false;
            }
            Self::NewCardInsertOrder => {
                m.default = Value::Number(0.0);
                m.choices = vec![0, 1];
            }
            Self::LeechAction => {
                m.default = Value::Number(1.0);
                m.choices = vec![0, 1];
            }
            Self::NewCardGatherPriority => {
                m.default = Value::Number(0.0);
                m.choices = vec![0, 5, 1, 2, 3, 4];
            }
            Self::NewCardSortOrder => {
                m.default = Value::Number(0.0);
                m.choices = vec![0, 1, 2, 3, 4];
            }
            Self::NewMix => {
                m.default = Value::Number(0.0);
                m.choices = vec![0, 1, 2];
            }
            Self::InterdayLearningMix => {
                m.default = Value::Number(0.0);
                m.choices = vec![0, 1, 2];
            }
            Self::ReviewOrder => {
                m.default = Value::Number(0.0);
                m.choices = vec![0, 1, 2, 3, 4, 6, 5, 7, 11, 12, 8, 9, 10];
            }
            Self::QuestionAction => {
                m.default = Value::Number(0.0);
                m.choices = vec![0, 1];
            }
            Self::AnswerAction => {
                m.default = Value::Number(0.0);
                m.choices = vec![0, 1, 2, 3, 4];
            }
            Self::BuryNew => m.default = Value::Boolean(false),
            Self::BuryReviews => m.default = Value::Boolean(false),
            Self::BuryInterdayLearning => m.default = Value::Boolean(false),
            Self::DisableAutoplay => m.default = Value::Boolean(false),
            Self::SkipQuestionWhenReplayingAnswer => m.default = Value::Boolean(false),
            Self::ShowTimer => m.default = Value::Boolean(false),
            Self::StopTimerOnAnswer => m.default = Value::Boolean(false),
            Self::WaitForAudio => m.default = Value::Boolean(true),
            Self::NewCardsIgnoreReviewLimit => m.default = Value::Boolean(false),
            Self::ApplyAllParentLimits => m.default = Value::Boolean(false),
            Self::LoadBalancerEnabled => m.default = Value::Boolean(true),
            Self::LearnSteps => m.default = Value::Steps(vec![1.0, 10.0]),
            Self::RelearnSteps => m.default = Value::Steps(vec![10.0]),
            Self::IgnoreRevlogsBeforeDate => m.default = Value::Text(String::new()),
            Self::NewLimit | Self::ReviewLimit | Self::NewLimitToday | Self::ReviewLimitToday => {
                m.minimum = Some(0.0);
                m.maximum = Some(9999.0);
                m.whole = true;
            }
            Self::DeckDesiredRetention => {
                m.minimum = Some(0.7);
                m.maximum = Some(0.99);
            }
            Self::Rollover => {
                m.default = Value::Number(4.0);
                m.minimum = Some(0.0);
                m.maximum = Some(23.0);
                m.whole = true;
            }
            Self::LearnAheadMinutes => {
                m.default = Value::Number(20.0);
                m.minimum = Some(0.0);
                m.maximum = Some(999.0);
                m.whole = true;
            }
        }
        if m.whole {
            m.step = Some(1.0);
        } else if matches!(self, Self::SecondsToShowQuestion | Self::SecondsToShowAnswer) {
            m.step = Some(0.1);
        }
        m
    }
}
pub fn read(fields: &DeckConfigMessage) -> BTreeMap<Field, Value> {
    Field::PRESET
        .iter()
        .map(|&field| {
            (
                field,
                match field {
                    Field::NewPerDay => Value::Number(fields.new_per_day as f64),
                    Field::ReviewsPerDay => Value::Number(fields.reviews_per_day as f64),
                    Field::DesiredRetention => Value::Number(fields.desired_retention as f64),
                    Field::MinimumLapseInterval => Value::Number(fields.minimum_lapse_interval as f64),
                    Field::LeechThreshold => Value::Number(fields.leech_threshold as f64),
                    Field::CapAnswerTimeToSecs => Value::Number(fields.cap_answer_time_to_secs as f64),
                    Field::SecondsToShowQuestion => Value::Number(fields.seconds_to_show_question as f64),
                    Field::SecondsToShowAnswer => Value::Number(fields.seconds_to_show_answer as f64),
                    Field::MaximumReviewInterval => Value::Number(fields.maximum_review_interval as f64),
                    Field::HistoricalRetention => Value::Number(fields.historical_retention as f64),
                    Field::NewCardInsertOrder => Value::Number(fields.new_card_insert_order as f64),
                    Field::LeechAction => Value::Number(fields.leech_action as f64),
                    Field::NewCardGatherPriority => Value::Number(fields.new_card_gather_priority as f64),
                    Field::NewCardSortOrder => Value::Number(fields.new_card_sort_order as f64),
                    Field::NewMix => Value::Number(fields.new_mix as f64),
                    Field::InterdayLearningMix => Value::Number(fields.interday_learning_mix as f64),
                    Field::ReviewOrder => Value::Number(fields.review_order as f64),
                    Field::QuestionAction => Value::Number(fields.question_action as f64),
                    Field::AnswerAction => Value::Number(fields.answer_action as f64),
                    Field::BuryNew => Value::Boolean(fields.bury_new),
                    Field::BuryReviews => Value::Boolean(fields.bury_reviews),
                    Field::BuryInterdayLearning => Value::Boolean(fields.bury_interday_learning),
                    Field::DisableAutoplay => Value::Boolean(fields.disable_autoplay),
                    Field::SkipQuestionWhenReplayingAnswer => {
                        Value::Boolean(fields.skip_question_when_replaying_answer)
                    }
                    Field::ShowTimer => Value::Boolean(fields.show_timer),
                    Field::StopTimerOnAnswer => Value::Boolean(fields.stop_timer_on_answer),
                    Field::WaitForAudio => Value::Boolean(fields.wait_for_audio),
                    Field::LearnSteps => Value::Steps(fields.learn_steps.clone()),
                    Field::RelearnSteps => Value::Steps(fields.relearn_steps.clone()),
                    Field::IgnoreRevlogsBeforeDate => Value::Text(fields.ignore_revlogs_before_date.clone()),
                    _ => unreachable!("preset field"),
                },
            )
        })
        .collect()
}
/// The caller validates first. This writes just the selected visible field
/// into a clone of the raw record; hidden and invalid unedited fields survive.
pub fn patch(fields: &mut DeckConfigMessage, field: Field, value: &Value) {
    match (field, value) {
        (Field::NewPerDay, Value::Number(v)) => fields.new_per_day = *v as u32,
        (Field::ReviewsPerDay, Value::Number(v)) => fields.reviews_per_day = *v as u32,
        (Field::DesiredRetention, Value::Number(v)) => fields.desired_retention = *v as f32,
        (Field::MinimumLapseInterval, Value::Number(v)) => fields.minimum_lapse_interval = *v as u32,
        (Field::LeechThreshold, Value::Number(v)) => fields.leech_threshold = *v as u32,
        (Field::CapAnswerTimeToSecs, Value::Number(v)) => fields.cap_answer_time_to_secs = *v as u32,
        (Field::SecondsToShowQuestion, Value::Number(v)) => fields.seconds_to_show_question = *v as f32,
        (Field::SecondsToShowAnswer, Value::Number(v)) => fields.seconds_to_show_answer = *v as f32,
        (Field::MaximumReviewInterval, Value::Number(v)) => fields.maximum_review_interval = *v as u32,
        (Field::HistoricalRetention, Value::Number(v)) => fields.historical_retention = *v as f32,
        (Field::NewCardInsertOrder, Value::Number(v)) => fields.new_card_insert_order = *v as i32,
        (Field::LeechAction, Value::Number(v)) => fields.leech_action = *v as i32,
        (Field::NewCardGatherPriority, Value::Number(v)) => fields.new_card_gather_priority = *v as i32,
        (Field::NewCardSortOrder, Value::Number(v)) => fields.new_card_sort_order = *v as i32,
        (Field::NewMix, Value::Number(v)) => fields.new_mix = *v as i32,
        (Field::InterdayLearningMix, Value::Number(v)) => fields.interday_learning_mix = *v as i32,
        (Field::ReviewOrder, Value::Number(v)) => fields.review_order = *v as i32,
        (Field::QuestionAction, Value::Number(v)) => fields.question_action = *v as i32,
        (Field::AnswerAction, Value::Number(v)) => fields.answer_action = *v as i32,
        (Field::BuryNew, Value::Boolean(v)) => fields.bury_new = *v,
        (Field::BuryReviews, Value::Boolean(v)) => fields.bury_reviews = *v,
        (Field::BuryInterdayLearning, Value::Boolean(v)) => fields.bury_interday_learning = *v,
        (Field::DisableAutoplay, Value::Boolean(v)) => fields.disable_autoplay = *v,
        (Field::SkipQuestionWhenReplayingAnswer, Value::Boolean(v)) => fields.skip_question_when_replaying_answer = *v,
        (Field::ShowTimer, Value::Boolean(v)) => fields.show_timer = *v,
        (Field::StopTimerOnAnswer, Value::Boolean(v)) => fields.stop_timer_on_answer = *v,
        (Field::WaitForAudio, Value::Boolean(v)) => fields.wait_for_audio = *v,
        (Field::LearnSteps, Value::Steps(v)) => fields.learn_steps = v.clone(),
        (Field::LearnSteps, Value::Text(v)) => {
            fields.learn_steps = super::steps_parse::parse(v, field, None).expect("validated steps")
        }
        (Field::RelearnSteps, Value::Steps(v)) => fields.relearn_steps = v.clone(),
        (Field::RelearnSteps, Value::Text(v)) => {
            fields.relearn_steps = super::steps_parse::parse(v, field, None).expect("validated steps")
        }
        (Field::IgnoreRevlogsBeforeDate, Value::Text(v)) => fields.ignore_revlogs_before_date = v.clone(),
        _ => unreachable!("validated visible field"),
    }
}
