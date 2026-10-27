//! Typed U49 exports, independent of slice/features. All collection work
//! runs on the runtime host's bounded job pool; no callback owns these locks.
use crate::{
    CoreError, callback,
    jobs::{JobHandle, wrap},
    mutations::MutationSummary,
    runtime,
};
use meguri_engine::sched::deck_options as engine;
use meguri_model::ids::DeckId;
use meguri_sched::options as values;
use std::sync::{Arc, Mutex};
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum OptionField {
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
impl From<values::Field> for OptionField {
    fn from(v: values::Field) -> Self {
        match v {
            values::Field::NewPerDay => Self::NewPerDay,
            values::Field::ReviewsPerDay => Self::ReviewsPerDay,
            values::Field::DesiredRetention => Self::DesiredRetention,
            values::Field::MinimumLapseInterval => Self::MinimumLapseInterval,
            values::Field::LeechThreshold => Self::LeechThreshold,
            values::Field::CapAnswerTimeToSecs => Self::CapAnswerTimeToSecs,
            values::Field::SecondsToShowQuestion => Self::SecondsToShowQuestion,
            values::Field::SecondsToShowAnswer => Self::SecondsToShowAnswer,
            values::Field::MaximumReviewInterval => Self::MaximumReviewInterval,
            values::Field::HistoricalRetention => Self::HistoricalRetention,
            values::Field::NewCardInsertOrder => Self::NewCardInsertOrder,
            values::Field::LeechAction => Self::LeechAction,
            values::Field::NewCardGatherPriority => Self::NewCardGatherPriority,
            values::Field::NewCardSortOrder => Self::NewCardSortOrder,
            values::Field::NewMix => Self::NewMix,
            values::Field::InterdayLearningMix => Self::InterdayLearningMix,
            values::Field::ReviewOrder => Self::ReviewOrder,
            values::Field::QuestionAction => Self::QuestionAction,
            values::Field::AnswerAction => Self::AnswerAction,
            values::Field::BuryNew => Self::BuryNew,
            values::Field::BuryReviews => Self::BuryReviews,
            values::Field::BuryInterdayLearning => Self::BuryInterdayLearning,
            values::Field::DisableAutoplay => Self::DisableAutoplay,
            values::Field::SkipQuestionWhenReplayingAnswer => Self::SkipQuestionWhenReplayingAnswer,
            values::Field::ShowTimer => Self::ShowTimer,
            values::Field::StopTimerOnAnswer => Self::StopTimerOnAnswer,
            values::Field::WaitForAudio => Self::WaitForAudio,
            values::Field::LearnSteps => Self::LearnSteps,
            values::Field::RelearnSteps => Self::RelearnSteps,
            values::Field::IgnoreRevlogsBeforeDate => Self::IgnoreRevlogsBeforeDate,
            values::Field::NewLimit => Self::NewLimit,
            values::Field::ReviewLimit => Self::ReviewLimit,
            values::Field::NewLimitToday => Self::NewLimitToday,
            values::Field::ReviewLimitToday => Self::ReviewLimitToday,
            values::Field::DeckDesiredRetention => Self::DeckDesiredRetention,
            values::Field::NewCardsIgnoreReviewLimit => Self::NewCardsIgnoreReviewLimit,
            values::Field::ApplyAllParentLimits => Self::ApplyAllParentLimits,
            values::Field::LoadBalancerEnabled => Self::LoadBalancerEnabled,
            values::Field::Rollover => Self::Rollover,
            values::Field::LearnAheadMinutes => Self::LearnAheadMinutes,
        }
    }
}
impl From<OptionField> for values::Field {
    fn from(v: OptionField) -> Self {
        match v {
            OptionField::NewPerDay => Self::NewPerDay,
            OptionField::ReviewsPerDay => Self::ReviewsPerDay,
            OptionField::DesiredRetention => Self::DesiredRetention,
            OptionField::MinimumLapseInterval => Self::MinimumLapseInterval,
            OptionField::LeechThreshold => Self::LeechThreshold,
            OptionField::CapAnswerTimeToSecs => Self::CapAnswerTimeToSecs,
            OptionField::SecondsToShowQuestion => Self::SecondsToShowQuestion,
            OptionField::SecondsToShowAnswer => Self::SecondsToShowAnswer,
            OptionField::MaximumReviewInterval => Self::MaximumReviewInterval,
            OptionField::HistoricalRetention => Self::HistoricalRetention,
            OptionField::NewCardInsertOrder => Self::NewCardInsertOrder,
            OptionField::LeechAction => Self::LeechAction,
            OptionField::NewCardGatherPriority => Self::NewCardGatherPriority,
            OptionField::NewCardSortOrder => Self::NewCardSortOrder,
            OptionField::NewMix => Self::NewMix,
            OptionField::InterdayLearningMix => Self::InterdayLearningMix,
            OptionField::ReviewOrder => Self::ReviewOrder,
            OptionField::QuestionAction => Self::QuestionAction,
            OptionField::AnswerAction => Self::AnswerAction,
            OptionField::BuryNew => Self::BuryNew,
            OptionField::BuryReviews => Self::BuryReviews,
            OptionField::BuryInterdayLearning => Self::BuryInterdayLearning,
            OptionField::DisableAutoplay => Self::DisableAutoplay,
            OptionField::SkipQuestionWhenReplayingAnswer => Self::SkipQuestionWhenReplayingAnswer,
            OptionField::ShowTimer => Self::ShowTimer,
            OptionField::StopTimerOnAnswer => Self::StopTimerOnAnswer,
            OptionField::WaitForAudio => Self::WaitForAudio,
            OptionField::LearnSteps => Self::LearnSteps,
            OptionField::RelearnSteps => Self::RelearnSteps,
            OptionField::IgnoreRevlogsBeforeDate => Self::IgnoreRevlogsBeforeDate,
            OptionField::NewLimit => Self::NewLimit,
            OptionField::ReviewLimit => Self::ReviewLimit,
            OptionField::NewLimitToday => Self::NewLimitToday,
            OptionField::ReviewLimitToday => Self::ReviewLimitToday,
            OptionField::DeckDesiredRetention => Self::DeckDesiredRetention,
            OptionField::NewCardsIgnoreReviewLimit => Self::NewCardsIgnoreReviewLimit,
            OptionField::ApplyAllParentLimits => Self::ApplyAllParentLimits,
            OptionField::LoadBalancerEnabled => Self::LoadBalancerEnabled,
            OptionField::Rollover => Self::Rollover,
            OptionField::LearnAheadMinutes => Self::LearnAheadMinutes,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum OptionIssueCode {
    OutOfRange,
    NotWhole,
    InvalidChoice,
    StepMalformed,
    StepZero,
    StepNegative,
    StepTooLong,
    InvalidDate,
    DateInFuture,
    StepsOneDayOrMore,
    ReviewsBelowTenTimesNew,
    RetentionLow,
    RetentionHigh,
    MaxIntervalShort,
    AnswerSecondsHigh,
}
impl From<values::Code> for OptionIssueCode {
    fn from(v: values::Code) -> Self {
        match v {
            values::Code::OutOfRange => Self::OutOfRange,
            values::Code::NotWhole => Self::NotWhole,
            values::Code::InvalidChoice => Self::InvalidChoice,
            values::Code::StepMalformed => Self::StepMalformed,
            values::Code::StepZero => Self::StepZero,
            values::Code::StepNegative => Self::StepNegative,
            values::Code::StepTooLong => Self::StepTooLong,
            values::Code::InvalidDate => Self::InvalidDate,
            values::Code::DateInFuture => Self::DateInFuture,
            values::Code::StepsOneDayOrMore => Self::StepsOneDayOrMore,
            values::Code::ReviewsBelowTenTimesNew => Self::ReviewsBelowTenTimesNew,
            values::Code::RetentionLow => Self::RetentionLow,
            values::Code::RetentionHigh => Self::RetentionHigh,
            values::Code::MaxIntervalShort => Self::MaxIntervalShort,
            values::Code::AnswerSecondsHigh => Self::AnswerSecondsHigh,
        }
    }
}
impl From<OptionIssueCode> for values::Code {
    fn from(v: OptionIssueCode) -> Self {
        match v {
            OptionIssueCode::OutOfRange => Self::OutOfRange,
            OptionIssueCode::NotWhole => Self::NotWhole,
            OptionIssueCode::InvalidChoice => Self::InvalidChoice,
            OptionIssueCode::StepMalformed => Self::StepMalformed,
            OptionIssueCode::StepZero => Self::StepZero,
            OptionIssueCode::StepNegative => Self::StepNegative,
            OptionIssueCode::StepTooLong => Self::StepTooLong,
            OptionIssueCode::InvalidDate => Self::InvalidDate,
            OptionIssueCode::DateInFuture => Self::DateInFuture,
            OptionIssueCode::StepsOneDayOrMore => Self::StepsOneDayOrMore,
            OptionIssueCode::ReviewsBelowTenTimesNew => Self::ReviewsBelowTenTimesNew,
            OptionIssueCode::RetentionLow => Self::RetentionLow,
            OptionIssueCode::RetentionHigh => Self::RetentionHigh,
            OptionIssueCode::MaxIntervalShort => Self::MaxIntervalShort,
            OptionIssueCode::AnswerSecondsHigh => Self::AnswerSecondsHigh,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum OptionSeverity {
    Error,
    Warning,
    StrongWarning,
}
impl From<values::Severity> for OptionSeverity {
    fn from(v: values::Severity) -> Self {
        match v {
            values::Severity::Error => Self::Error,
            values::Severity::Warning => Self::Warning,
            values::Severity::StrongWarning => Self::StrongWarning,
        }
    }
}
impl From<OptionSeverity> for values::Severity {
    fn from(v: OptionSeverity) -> Self {
        match v {
            OptionSeverity::Error => Self::Error,
            OptionSeverity::Warning => Self::Warning,
            OptionSeverity::StrongWarning => Self::StrongWarning,
        }
    }
}
#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum OptionValue {
    Number { value: f64 },
    Boolean { value: bool },
    Text { value: String },
    Steps { minutes: Vec<f32> },
    None,
}
impl From<values::Value> for OptionValue {
    fn from(v: values::Value) -> Self {
        match v {
            values::Value::Number(value) => Self::Number { value },
            values::Value::Boolean(value) => Self::Boolean { value },
            values::Value::Text(value) => Self::Text { value },
            values::Value::Steps(minutes) => Self::Steps { minutes },
            values::Value::None => Self::None,
        }
    }
}
impl From<OptionValue> for values::Value {
    fn from(v: OptionValue) -> Self {
        match v {
            OptionValue::Number { value } => Self::Number(value),
            OptionValue::Boolean { value } => Self::Boolean(value),
            OptionValue::Text { value } => Self::Text(value),
            OptionValue::Steps { minutes } => Self::Steps(minutes),
            OptionValue::None => Self::None,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct OptionIssue {
    pub field: OptionField,
    pub preset: Option<i64>,
    pub code: OptionIssueCode,
    pub severity: OptionSeverity,
    pub params: std::collections::HashMap<String, String>,
}
impl From<values::Issue> for OptionIssue {
    fn from(i: values::Issue) -> Self {
        Self {
            field: i.field.into(),
            preset: i.preset,
            code: i.code.into(),
            severity: i.severity.into(),
            params: i.params.into_iter().collect(),
        }
    }
}
impl From<OptionIssue> for values::Issue {
    fn from(i: OptionIssue) -> Self {
        Self {
            field: i.field.into(),
            preset: i.preset,
            code: i.code.into(),
            severity: i.severity.into(),
            params: i.params.into_iter().collect(),
        }
    }
}
#[derive(Clone, Debug, uniffi::Record)]
pub struct OptionFieldValue {
    pub field: OptionField,
    pub value: OptionValue,
}
fn fields(v: &std::collections::BTreeMap<values::Field, values::Value>) -> Vec<OptionFieldValue> {
    v.iter()
        .map(|(f, v)| OptionFieldValue {
            field: (*f).into(),
            value: v.clone().into(),
        })
        .collect()
}
#[derive(Clone, Debug, uniffi::Record)]
pub struct OptionMetadata {
    pub field: OptionField,
    pub default_value: OptionValue,
    pub minimum: Option<f64>,
    pub maximum: Option<f64>,
    pub step: Option<f64>,
    pub whole: bool,
    pub choices: Vec<i32>,
    pub global: bool,
    pub optional: bool,
}
impl From<values::Metadata> for OptionMetadata {
    fn from(m: values::Metadata) -> Self {
        Self {
            field: m.field.into(),
            default_value: m.default.into(),
            minimum: m.minimum,
            maximum: m.maximum,
            step: m.step,
            whole: m.whole,
            choices: m.choices,
            global: m.global,
            optional: m.optional,
        }
    }
}
#[derive(Clone, Debug, uniffi::Record)]
pub struct OptionPreset {
    pub key: i64,
    pub name: String,
    pub use_count: u64,
    pub values: Vec<OptionFieldValue>,
}
#[derive(Clone, Debug, uniffi::Record)]
pub struct OptionTodayLimit {
    pub limit: u32,
    pub today: u32,
    pub active: bool,
}
impl From<engine::TodayLimit> for OptionTodayLimit {
    fn from(v: engine::TodayLimit) -> Self {
        Self {
            limit: v.limit,
            today: v.today,
            active: v.active,
        }
    }
}
#[derive(Clone, Debug, uniffi::Record)]
pub struct OptionIgnoreCount {
    pub included: u64,
    pub total: u64,
    pub warning: bool,
}
impl From<meguri_storage::queries::revlog_fsrs::IncludedCount> for OptionIgnoreCount {
    fn from(v: meguri_storage::queries::revlog_fsrs::IncludedCount) -> Self {
        Self {
            included: v.included,
            total: v.total,
            warning: v.warning(),
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum OptionLimitScope {
    Preset,
    ThisDeck,
    TodayOnly,
}
impl From<engine::LimitScope> for OptionLimitScope {
    fn from(v: engine::LimitScope) -> Self {
        match v {
            engine::LimitScope::Preset => Self::Preset,
            engine::LimitScope::ThisDeck => Self::ThisDeck,
            engine::LimitScope::TodayOnly => Self::TodayOnly,
        }
    }
}
impl From<OptionLimitScope> for engine::LimitScope {
    fn from(v: OptionLimitScope) -> Self {
        match v {
            OptionLimitScope::Preset => Self::Preset,
            OptionLimitScope::ThisDeck => Self::ThisDeck,
            OptionLimitScope::TodayOnly => Self::TodayOnly,
        }
    }
}
#[derive(Clone, Debug, uniffi::Record)]
pub struct DeckOptionsSnapshot {
    pub new_limit_scope: OptionLimitScope,
    pub review_limit_scope: OptionLimitScope,
    pub retention_this_deck: bool,
    pub deck: i64,
    pub name: String,
    pub assigned_preset: i64,
    pub selected_preset: i64,
    pub preset_missing: bool,
    pub today: u32,
    pub today_date: String,
    pub presets: Vec<OptionPreset>,
    pub deck_values: Vec<OptionFieldValue>,
    pub globals: Vec<OptionFieldValue>,
    pub metadata: Vec<OptionMetadata>,
    pub new_today: Option<OptionTodayLimit>,
    pub review_today: Option<OptionTodayLimit>,
    pub schema_modified: bool,
    pub ignore_count: Option<OptionIgnoreCount>,
    pub unsaved_changes: bool,
    pub subtree: bool,
    pub effective_new_limit: u32,
    pub effective_review_limit: u32,
    pub effective_retention: f32,
}
fn view(d: &engine::Draft) -> DeckOptionsSnapshot {
    let effective = |a, b, p| {
        [
            d.deck_values.get(&a),
            d.deck_values.get(&b),
            d.presets
                .iter()
                .find(|p| p.key == d.selected)
                .and_then(|r| r.values.get(&p)),
        ]
        .into_iter()
        .flatten()
        .find_map(|v| match v {
            values::Value::Number(v) => Some(*v),
            _ => None,
        })
        .unwrap_or(0.0)
    };
    let retention = d
        .deck_values
        .get(&values::Field::DeckDesiredRetention)
        .and_then(|v| match v {
            values::Value::Number(v) => Some(*v),
            _ => None,
        })
        .unwrap_or_else(|| {
            d.presets
                .iter()
                .find(|p| p.key == d.selected)
                .and_then(|p| p.values.get(&values::Field::DesiredRetention))
                .and_then(|v| match v {
                    values::Value::Number(v) => Some(*v),
                    _ => None,
                })
                .unwrap_or(0.9)
        });
    DeckOptionsSnapshot {
        new_limit_scope: d.limit_scope(false).into(),
        review_limit_scope: d.limit_scope(true).into(),
        retention_this_deck: !matches!(
            d.deck_values.get(&values::Field::DeckDesiredRetention),
            Some(values::Value::None) | None
        ),
        deck: d.base.deck.0,
        name: d.base.name.clone(),
        assigned_preset: d.base.assigned,
        selected_preset: d.selected,
        preset_missing: d.base.preset_missing,
        today: d.base.today,
        today_date: d.base.today_date.clone(),
        presets: d
            .presets
            .iter()
            .map(|p| OptionPreset {
                key: p.key,
                name: p.name.clone(),
                use_count: d.use_count(p.key).unwrap_or(0),
                values: fields(&p.values),
            })
            .collect(),
        deck_values: fields(&d.deck_values),
        globals: fields(&d.globals),
        metadata: values::Field::PRESET
            .iter()
            .chain(values::Field::DECK)
            .chain(values::Field::GLOBAL)
            .map(|f| f.metadata().into())
            .collect(),
        new_today: d.base.new_today.clone().map(Into::into),
        review_today: d.base.review_today.clone().map(Into::into),
        schema_modified: d.base.schema_modified,
        ignore_count: d.base.ignore_count.map(Into::into),
        unsaved_changes: d.has_changes(),
        subtree: d.subtree,
        effective_new_limit: effective(
            values::Field::NewLimitToday,
            values::Field::NewLimit,
            values::Field::NewPerDay,
        ) as u32,
        effective_review_limit: effective(
            values::Field::ReviewLimitToday,
            values::Field::ReviewLimit,
            values::Field::ReviewsPerDay,
        ) as u32,
        effective_retention: retention as f32,
    }
}
#[derive(Clone, Debug, uniffi::Enum)]
pub enum DeckOptionsEdit {
    SetLimit {
        review: bool,
        scope: OptionLimitScope,
        value: f64,
    },
    SetRetention {
        this_deck: bool,
        value: f64,
    },
    Set {
        preset: Option<i64>,
        field: OptionField,
        value: OptionValue,
    },
    Select {
        preset: i64,
    },
    Add {
        name: String,
    },
    Clone {
        name: String,
    },
    Rename {
        name: String,
    },
    Remove {
        preset: i64,
    },
    RestoreDefault {
        preset: Option<i64>,
        field: OptionField,
    },
    Configure {
        subtree: bool,
    },
}
#[derive(Debug, uniffi::Object)]
pub struct DeckOptionsEditor {
    collection: meguri_engine::Collection,
    draft: Mutex<engine::Draft>,
}
#[uniffi::export]
impl DeckOptionsEditor {
    pub fn snapshot(&self) -> Result<DeckOptionsSnapshot, CoreError> {
        callback::enter("DeckOptionsEditor::snapshot");
        Ok(view(&self.draft.lock().unwrap()))
    }
    pub fn edit(&self, edit: DeckOptionsEdit) -> Result<DeckOptionsSnapshot, CoreError> {
        callback::enter("DeckOptionsEditor::edit");
        let mut guard = self.draft.lock().unwrap();
        let mut d = guard.clone();
        match edit {
            DeckOptionsEdit::SetLimit { review, scope, value } => d.set_limit(review, scope.into(), value)?,
            DeckOptionsEdit::SetRetention { this_deck, value } => d.set_retention(this_deck, value)?,
            DeckOptionsEdit::Set { preset, field, value } => d.edit(preset, field.into(), value.into())?,
            DeckOptionsEdit::Select { preset } => d.select(preset)?,
            DeckOptionsEdit::Add { name } => {
                engine::stage_add(&self.collection, &mut d, &name, false)?;
            }
            DeckOptionsEdit::Clone { name } => {
                engine::stage_add(&self.collection, &mut d, &name, true)?;
            }
            DeckOptionsEdit::Rename { name } => engine::stage_rename(&self.collection, &mut d, &name)?,
            DeckOptionsEdit::Remove { preset } => d.remove(preset)?,
            DeckOptionsEdit::RestoreDefault { preset, field } => d.restore_default(preset, field.into())?,
            DeckOptionsEdit::Configure { subtree } => {
                d.subtree = subtree;
            }
        }
        let snapshot = view(&d);
        *guard = d;
        Ok(snapshot)
    }
    pub fn validate(&self) -> Result<Vec<OptionIssue>, CoreError> {
        callback::enter("DeckOptionsEditor::validate");
        let d = self.draft.lock().unwrap();
        Ok(d.validate(&d.base.today_date).into_iter().map(Into::into).collect())
    }
}
#[derive(Clone, Debug, uniffi::Record)]
pub struct ParsedOptionSteps {
    pub minutes: Vec<f32>,
    pub issues: Vec<OptionIssue>,
}
#[uniffi::export]
pub fn parse_option_steps(text: String) -> Result<ParsedOptionSteps, CoreError> {
    callback::enter("parse_option_steps");
    Ok(
        match values::steps_parse::parse(&text, values::Field::LearnSteps, None) {
            Ok(minutes) => ParsedOptionSteps {
                minutes,
                issues: vec![],
            },
            Err(issues) => ParsedOptionSteps {
                minutes: vec![],
                issues: issues.into_iter().map(Into::into).collect(),
            },
        },
    )
}
#[uniffi::export]
pub fn format_option_steps(minutes: Vec<f32>) -> Result<String, CoreError> {
    callback::enter("format_option_steps");
    Ok(values::steps_parse::format(&minutes))
}
#[derive(Clone, Debug, uniffi::Record)]
pub struct DeckOptionsMutation {
    pub summary: MutationSummary,
    pub preset: i64,
    pub processed: u64,
    pub changed: u64,
    pub skipped_presets: Vec<i64>,
}
#[derive(Clone, Debug, uniffi::Record)]
pub struct SchedulingPreferences {
    pub rollover: u8,
    pub learn_ahead_minutes: u32,
    pub load_balancer: bool,
    pub metadata: Vec<OptionMetadata>,
}
#[derive(Clone, Debug, uniffi::Record)]
pub struct SchedulingPreferencesChange {
    pub rollover: Option<f64>,
    pub learn_ahead_minutes: Option<f64>,
    pub load_balancer: Option<bool>,
}
#[derive(Debug, uniffi::Object)]
pub struct DeckOptionsReadJob {
    result: Arc<Mutex<Option<Arc<DeckOptionsEditor>>>>,
}
#[uniffi::export]
impl DeckOptionsReadJob {
    pub fn result(&self) -> Result<Option<Arc<DeckOptionsEditor>>, CoreError> {
        callback::enter("DeckOptionsReadJob::result");
        Ok(self.result.lock().unwrap().clone())
    }
}
#[derive(Clone, Debug, uniffi::Record)]
pub struct DeckOptionsReadJobStart {
    pub handle: JobHandle,
    pub result: Arc<DeckOptionsReadJob>,
}
fn job_deckoptionsread(
    work: impl FnOnce(&meguri_engine::jobs::JobContext) -> Result<Arc<DeckOptionsEditor>, meguri_engine::EngineError>
    + Send
    + 'static,
) -> Result<DeckOptionsReadJobStart, CoreError> {
    let result = Arc::new(Mutex::new(None));
    let out = result.clone();
    let handle = runtime::engine()?.start_job(meguri_engine::JobKind::BulkOp, move |j| {
        j.cancel.check()?;
        let value = work(j)?;
        *out.lock().unwrap() = Some(value);
        Ok(())
    })?;
    Ok(DeckOptionsReadJobStart {
        handle: wrap(handle),
        result: Arc::new(DeckOptionsReadJob { result }),
    })
}
#[derive(Debug, uniffi::Object)]
pub struct DeckOptionsSaveJob {
    result: Arc<Mutex<Option<DeckOptionsMutation>>>,
}
#[uniffi::export]
impl DeckOptionsSaveJob {
    pub fn result(&self) -> Result<Option<DeckOptionsMutation>, CoreError> {
        callback::enter("DeckOptionsSaveJob::result");
        Ok(self.result.lock().unwrap().clone())
    }
}
#[derive(Clone, Debug, uniffi::Record)]
pub struct DeckOptionsSaveJobStart {
    pub handle: JobHandle,
    pub result: Arc<DeckOptionsSaveJob>,
}
fn job_deckoptionssave(
    work: impl FnOnce(&meguri_engine::jobs::JobContext) -> Result<DeckOptionsMutation, meguri_engine::EngineError>
    + Send
    + 'static,
) -> Result<DeckOptionsSaveJobStart, CoreError> {
    let result = Arc::new(Mutex::new(None));
    let out = result.clone();
    let handle = runtime::engine()?.start_job(meguri_engine::JobKind::BulkOp, move |j| {
        j.cancel.check()?;
        let value = work(j)?;
        *out.lock().unwrap() = Some(value);
        Ok(())
    })?;
    Ok(DeckOptionsSaveJobStart {
        handle: wrap(handle),
        result: Arc::new(DeckOptionsSaveJob { result }),
    })
}
#[derive(Debug, uniffi::Object)]
pub struct OptionIgnoreCountJob {
    result: Arc<Mutex<Option<OptionIgnoreCount>>>,
}
#[uniffi::export]
impl OptionIgnoreCountJob {
    pub fn result(&self) -> Result<Option<OptionIgnoreCount>, CoreError> {
        callback::enter("OptionIgnoreCountJob::result");
        Ok(self.result.lock().unwrap().clone())
    }
}
#[derive(Clone, Debug, uniffi::Record)]
pub struct OptionIgnoreCountJobStart {
    pub handle: JobHandle,
    pub result: Arc<OptionIgnoreCountJob>,
}
fn job_optionignorecount(
    work: impl FnOnce(&meguri_engine::jobs::JobContext) -> Result<OptionIgnoreCount, meguri_engine::EngineError>
    + Send
    + 'static,
) -> Result<OptionIgnoreCountJobStart, CoreError> {
    let result = Arc::new(Mutex::new(None));
    let out = result.clone();
    let handle = runtime::engine()?.start_job(meguri_engine::JobKind::BulkOp, move |j| {
        j.cancel.check()?;
        let value = work(j)?;
        *out.lock().unwrap() = Some(value);
        Ok(())
    })?;
    Ok(OptionIgnoreCountJobStart {
        handle: wrap(handle),
        result: Arc::new(OptionIgnoreCountJob { result }),
    })
}
#[derive(Debug, uniffi::Object)]
pub struct SchedulingPreferencesJob {
    result: Arc<Mutex<Option<SchedulingPreferences>>>,
}
#[uniffi::export]
impl SchedulingPreferencesJob {
    pub fn result(&self) -> Result<Option<SchedulingPreferences>, CoreError> {
        callback::enter("SchedulingPreferencesJob::result");
        Ok(self.result.lock().unwrap().clone())
    }
}
#[derive(Clone, Debug, uniffi::Record)]
pub struct SchedulingPreferencesJobStart {
    pub handle: JobHandle,
    pub result: Arc<SchedulingPreferencesJob>,
}
fn job_schedulingpreferences(
    work: impl FnOnce(&meguri_engine::jobs::JobContext) -> Result<SchedulingPreferences, meguri_engine::EngineError>
    + Send
    + 'static,
) -> Result<SchedulingPreferencesJobStart, CoreError> {
    let result = Arc::new(Mutex::new(None));
    let out = result.clone();
    let handle = runtime::engine()?.start_job(meguri_engine::JobKind::BulkOp, move |j| {
        j.cancel.check()?;
        let value = work(j)?;
        *out.lock().unwrap() = Some(value);
        Ok(())
    })?;
    Ok(SchedulingPreferencesJobStart {
        handle: wrap(handle),
        result: Arc::new(SchedulingPreferencesJob { result }),
    })
}
#[derive(Debug, uniffi::Object)]
pub struct SchedulingPreferencesSaveJob {
    result: Arc<Mutex<Option<MutationSummary>>>,
}
#[uniffi::export]
impl SchedulingPreferencesSaveJob {
    pub fn result(&self) -> Result<Option<MutationSummary>, CoreError> {
        callback::enter("SchedulingPreferencesSaveJob::result");
        Ok(self.result.lock().unwrap().clone())
    }
}
#[derive(Clone, Debug, uniffi::Record)]
pub struct SchedulingPreferencesSaveJobStart {
    pub handle: JobHandle,
    pub result: Arc<SchedulingPreferencesSaveJob>,
}
fn job_schedulingpreferencessave(
    work: impl FnOnce(&meguri_engine::jobs::JobContext) -> Result<MutationSummary, meguri_engine::EngineError>
    + Send
    + 'static,
) -> Result<SchedulingPreferencesSaveJobStart, CoreError> {
    let result = Arc::new(Mutex::new(None));
    let out = result.clone();
    let handle = runtime::engine()?.start_job(meguri_engine::JobKind::BulkOp, move |j| {
        j.cancel.check()?;
        let value = work(j)?;
        *out.lock().unwrap() = Some(value);
        Ok(())
    })?;
    Ok(SchedulingPreferencesSaveJobStart {
        handle: wrap(handle),
        result: Arc::new(SchedulingPreferencesSaveJob { result }),
    })
}
#[uniffi::export]
pub fn start_deck_options(deck: i64) -> Result<DeckOptionsReadJobStart, CoreError> {
    callback::enter("start_deck_options");
    let host = runtime::host()?;
    job_deckoptionsread(move |j| {
        let collection = host.collection()?;
        let draft = engine::Draft::new(engine::snapshot(&collection, DeckId(deck))?);
        j.cancel.check()?;
        Ok(Arc::new(DeckOptionsEditor {
            collection,
            draft: Mutex::new(draft),
        }))
    })
}
#[uniffi::export]
pub fn start_save_deck_options(editor: Arc<DeckOptionsEditor>) -> Result<DeckOptionsSaveJobStart, CoreError> {
    callback::enter("start_save_deck_options");
    let draft = editor.draft.lock().unwrap().clone();
    job_deckoptionssave(move |j| {
        let r = engine::save(
            &editor.collection,
            draft,
            &meguri_engine::tags::RunOptions {
                cancel: Some(j.cancel.clone()),
                progress: Some(j.progress.clone()),
            },
        )?;
        Ok(DeckOptionsMutation {
            summary: MutationSummary::of(&r),
            preset: r.value.preset.0,
            processed: r.value.memory.processed as u64,
            changed: r.value.memory.changed as u64,
            skipped_presets: r.value.memory.skipped_presets.iter().map(|p| p.0).collect(),
        })
    })
}
#[uniffi::export]
pub fn start_option_ignore_count(
    editor: Arc<DeckOptionsEditor>,
    preset: i64,
    date: String,
) -> Result<OptionIgnoreCountJobStart, CoreError> {
    callback::enter("start_option_ignore_count");
    let draft = editor.draft.lock().unwrap().clone();
    job_optionignorecount(move |j| {
        let count = engine::ignored_before_count(&editor.collection, &draft, preset, &date)?;
        j.cancel.check()?;
        Ok(count.into())
    })
}
#[uniffi::export]
pub fn start_scheduling_preferences() -> Result<SchedulingPreferencesJobStart, CoreError> {
    callback::enter("start_scheduling_preferences");
    let host = runtime::host()?;
    job_schedulingpreferences(move |_| {
        let p = engine::preferences(&host.collection()?)?;
        Ok(SchedulingPreferences {
            rollover: p.rollover,
            learn_ahead_minutes: p.learn_ahead_minutes,
            load_balancer: p.load_balancer,
            metadata: [
                values::Field::Rollover,
                values::Field::LearnAheadMinutes,
                values::Field::LoadBalancerEnabled,
            ]
            .into_iter()
            .map(|f| f.metadata().into())
            .collect(),
        })
    })
}
#[uniffi::export]
pub fn start_save_scheduling_preferences(
    change: SchedulingPreferencesChange,
) -> Result<SchedulingPreferencesSaveJobStart, CoreError> {
    callback::enter("start_save_scheduling_preferences");
    let host = runtime::host()?;
    job_schedulingpreferencessave(move |j| {
        let r = engine::save_preferences(
            &host.collection()?,
            engine::PreferencesChange {
                rollover: change.rollover,
                learn_ahead_minutes: change.learn_ahead_minutes,
                load_balancer: change.load_balancer,
            },
            &meguri_engine::tags::RunOptions {
                cancel: Some(j.cancel.clone()),
                progress: Some(j.progress.clone()),
            },
        )?;
        Ok(MutationSummary::of(&r))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn typed_validation_errors_survive_the_engine_job_error_roundtrip() {
        let issues = values::steps_parse::parse("0m -5 5x", values::Field::LearnSteps, Some(-7)).unwrap_err();
        let engine = meguri_engine::EngineError::DeckOptionsInvalid(issues);
        assert_eq!(
            meguri_engine::EngineError::from(CoreError::from(engine.clone())),
            engine
        );
        assert_eq!(parse_option_steps("1m 10m".into()).unwrap().minutes, vec![1., 10.]);
        assert_eq!(format_option_steps(vec![2160., 525600.]).unwrap(), "36h 365d");
    }
}

#[uniffi::export]
pub fn validate_scheduling_preferences(change: SchedulingPreferencesChange) -> Result<Vec<OptionIssue>, CoreError> {
    callback::enter("validate_scheduling_preferences");
    Ok(engine::validate_preferences(&engine::PreferencesChange {
        rollover: change.rollover,
        learn_ahead_minutes: change.learn_ahead_minutes,
        load_balancer: change.load_balancer,
    })
    .into_iter()
    .map(Into::into)
    .collect())
}
