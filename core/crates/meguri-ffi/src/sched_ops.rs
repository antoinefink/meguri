//! Typed U48 exports. Work and pending-answer drains stay on bounded jobs.
use crate::{
    CoreError, callback,
    jobs::JobHandle,
    mutations::MutationSummary,
    review_session::{ReviewHeadDiff, ReviewRating},
    runtime,
};
use meguri_engine::{SessionId, sched::ops};
use meguri_model::ids::{CardId, DeckId, NoteId};
use std::sync::{Arc, Mutex};

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum InvalidDaysKind {
    Empty,
    Malformed,
    TooLarge,
}
impl From<ops::InvalidDays> for InvalidDaysKind {
    fn from(v: ops::InvalidDays) -> Self {
        match v {
            ops::InvalidDays::Empty => Self::Empty,
            ops::InvalidDays::Malformed => Self::Malformed,
            ops::InvalidDays::TooLarge => Self::TooLarge,
        }
    }
}
impl From<InvalidDaysKind> for ops::InvalidDays {
    fn from(v: InvalidDaysKind) -> Self {
        match v {
            InvalidDaysKind::Empty => Self::Empty,
            InvalidDaysKind::Malformed => Self::Malformed,
            InvalidDaysKind::TooLarge => Self::TooLarge,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum SchedOpRefusal {
    InvalidDays { kind: InvalidDaysKind },
    HomeDeckFiltered { deck: i64 },
    InFilteredDeck { card: i64 },
    NotFiltered { deck: i64 },
    PresetUnsupported { preset_ids: Vec<i64> },
}
impl From<ops::Refusal> for SchedOpRefusal {
    fn from(v: ops::Refusal) -> Self {
        match v {
            ops::Refusal::InvalidDays(kind) => Self::InvalidDays { kind: kind.into() },
            ops::Refusal::HomeDeckFiltered { deck } => Self::HomeDeckFiltered { deck: deck.0 },
            ops::Refusal::InFilteredDeck { card } => Self::InFilteredDeck { card: card.0 },
            ops::Refusal::NotFiltered { deck } => Self::NotFiltered { deck: deck.0 },
            ops::Refusal::PresetUnsupported { preset_ids } => Self::PresetUnsupported {
                preset_ids: preset_ids.into_iter().map(|id| id.0).collect(),
            },
        }
    }
}
impl From<SchedOpRefusal> for ops::Refusal {
    fn from(v: SchedOpRefusal) -> Self {
        match v {
            SchedOpRefusal::InvalidDays { kind } => Self::InvalidDays(kind.into()),
            SchedOpRefusal::HomeDeckFiltered { deck } => Self::HomeDeckFiltered { deck: DeckId(deck) },
            SchedOpRefusal::InFilteredDeck { card } => Self::InFilteredDeck { card: CardId(card) },
            SchedOpRefusal::NotFiltered { deck } => Self::NotFiltered { deck: DeckId(deck) },
            SchedOpRefusal::PresetUnsupported { preset_ids } => Self::PresetUnsupported {
                preset_ids: preset_ids.into_iter().map(meguri_model::ids::DeckConfigId).collect(),
            },
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct DueDays {
    pub minimum: u32,
    pub maximum: u32,
    pub reset_interval: bool,
}
#[uniffi::export]
pub fn parse_due_days(text: String) -> Result<DueDays, CoreError> {
    callback::enter("parse_due_days");
    let days = ops::Days::parse(&text).map_err(|kind| CoreError::SchedOpRefused {
        refusal: SchedOpRefusal::InvalidDays { kind: kind.into() },
    })?;
    Ok(DueDays {
        minimum: days.minimum,
        maximum: days.maximum,
        reset_interval: days.reset_interval,
    })
}
#[derive(Clone, Debug, uniffi::Enum)]
pub enum SchedTargets {
    Cards { ids: Vec<i64> },
    Notes { ids: Vec<i64> },
}
impl From<SchedTargets> for ops::Targets {
    fn from(v: SchedTargets) -> Self {
        match v {
            SchedTargets::Cards { ids } => Self::Cards(ids.into_iter().map(CardId).collect()),
            SchedTargets::Notes { ids } => Self::Notes(ids.into_iter().map(NoteId).collect()),
        }
    }
}
#[derive(Clone, Copy, Debug, uniffi::Enum)]
pub enum SchedContext {
    Reviewer,
    Browser,
}
impl From<SchedContext> for ops::Context {
    fn from(v: SchedContext) -> Self {
        match v {
            SchedContext::Reviewer => Self::Reviewer,
            SchedContext::Browser => Self::Browser,
        }
    }
}
#[derive(Clone, Copy, Debug, uniffi::Enum)]
pub enum DeckUnburyMode {
    All,
    Manual,
    Siblings,
}
impl From<DeckUnburyMode> for ops::UnburyMode {
    fn from(v: DeckUnburyMode) -> Self {
        match v {
            DeckUnburyMode::All => Self::All,
            DeckUnburyMode::Manual => Self::Manual,
            DeckUnburyMode::Siblings => Self::Siblings,
        }
    }
}
#[derive(Clone, Debug, uniffi::Enum)]
pub enum SchedOperation {
    Bury {
        targets: SchedTargets,
    },
    Suspend {
        targets: SchedTargets,
    },
    Unsuspend {
        cards: Vec<i64>,
    },
    UnburyDeck {
        deck: i64,
        mode: DeckUnburyMode,
    },
    SetDue {
        cards: Vec<i64>,
        days: String,
        context: Option<SchedContext>,
    },
    Reset {
        cards: Vec<i64>,
        restore_position: bool,
        reset_counts: bool,
        context: Option<SchedContext>,
    },
    GradeNow {
        cards: Vec<i64>,
        rating: ReviewRating,
    },
    EmptyFiltered {
        deck: i64,
    },
}
impl From<SchedOperation> for ops::Request {
    fn from(v: SchedOperation) -> Self {
        match v {
            SchedOperation::Bury { targets } => Self::Bury(targets.into()),
            SchedOperation::Suspend { targets } => Self::Suspend(targets.into()),
            SchedOperation::Unsuspend { cards } => Self::Unsuspend(cards.into_iter().map(CardId).collect()),
            SchedOperation::UnburyDeck { deck, mode } => Self::UnburyDeck {
                deck: DeckId(deck),
                mode: mode.into(),
            },
            SchedOperation::SetDue { cards, days, context } => Self::SetDue {
                cards: cards.into_iter().map(CardId).collect(),
                days,
                context: context.map(Into::into),
            },
            SchedOperation::Reset {
                cards,
                restore_position,
                reset_counts,
                context,
            } => Self::Reset {
                cards: cards.into_iter().map(CardId).collect(),
                restore_position,
                reset_counts,
                context: context.map(Into::into),
            },
            SchedOperation::GradeNow { cards, rating } => Self::GradeNow {
                cards: cards.into_iter().map(CardId).collect(),
                rating: rating.into(),
            },
            SchedOperation::EmptyFiltered { deck } => Self::EmptyFiltered { deck: DeckId(deck) },
        }
    }
}
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct SchedMutation {
    pub summary: MutationSummary,
    pub count: u64,
    pub head_diff: Option<ReviewHeadDiff>,
}
impl From<meguri_engine::OpResult<ops::ResultValue>> for SchedMutation {
    fn from(r: meguri_engine::OpResult<ops::ResultValue>) -> Self {
        Self {
            summary: MutationSummary::of(&r),
            count: r.value.count as u64,
            head_diff: r.value.head_diff.map(Into::into),
        }
    }
}
#[derive(Debug, uniffi::Object)]
pub struct SchedOpJob {
    result: Arc<Mutex<Option<SchedMutation>>>,
}
#[uniffi::export]
impl SchedOpJob {
    pub fn result(&self) -> Result<Option<SchedMutation>, CoreError> {
        callback::enter("SchedOpJob::result");
        Ok(self.result.lock().unwrap().clone())
    }
}
#[derive(Clone, Debug, uniffi::Record)]
pub struct SchedOpJobStart {
    pub handle: JobHandle,
    pub result: Arc<SchedOpJob>,
}
#[uniffi::export]
pub fn start_sched_op(operation: SchedOperation, session_id: Option<u64>) -> Result<SchedOpJobStart, CoreError> {
    callback::enter("start_sched_op");
    let host = runtime::host()?;
    let result = Arc::new(Mutex::new(None));
    let out = result.clone();
    let handle = runtime::engine()?.start_job(meguri_engine::JobKind::BulkOp, move |j| {
        let value = host.review().sched_op(
            operation.into(),
            session_id.map(SessionId),
            &meguri_engine::tags::RunOptions {
                cancel: Some(j.cancel.clone()),
                progress: Some(j.progress.clone()),
            },
        )?;
        *out.lock().unwrap() = Some(value.into());
        Ok(())
    })?;
    Ok(SchedOpJobStart {
        handle: crate::jobs::wrap(handle),
        result: Arc::new(SchedOpJob { result }),
    })
}
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct BuriedKinds {
    pub manual: bool,
    pub siblings: bool,
}
#[derive(Debug, uniffi::Object)]
pub struct BuriedKindsJob {
    result: Arc<Mutex<Option<BuriedKinds>>>,
}
#[uniffi::export]
impl BuriedKindsJob {
    pub fn result(&self) -> Result<Option<BuriedKinds>, CoreError> {
        callback::enter("BuriedKindsJob::result");
        Ok(self.result.lock().unwrap().clone())
    }
}
#[derive(Clone, Debug, uniffi::Record)]
pub struct BuriedKindsJobStart {
    pub handle: JobHandle,
    pub result: Arc<BuriedKindsJob>,
}
#[uniffi::export]
pub fn start_buried_kinds(deck: i64) -> Result<BuriedKindsJobStart, CoreError> {
    callback::enter("start_buried_kinds");
    let host = runtime::host()?;
    let result = Arc::new(Mutex::new(None));
    let out = result.clone();
    let handle = runtime::engine()?.start_job(meguri_engine::JobKind::BulkOp, move |j| {
        j.cancel.check()?;
        let kinds = ops::buried_kinds(&host.collection()?, DeckId(deck))?;
        *out.lock().unwrap() = Some(BuriedKinds {
            manual: kinds.manual,
            siblings: kinds.siblings,
        });
        Ok(())
    })?;
    Ok(BuriedKindsJobStart {
        handle: crate::jobs::wrap(handle),
        result: Arc::new(BuriedKindsJob { result }),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parser_and_all_typed_refusals_survive_the_job_error_roundtrip() {
        assert_eq!(
            parse_due_days("5-2!".into()).unwrap(),
            DueDays {
                minimum: 2,
                maximum: 5,
                reset_interval: true
            }
        );
        for (text, kind) in [
            ("", InvalidDaysKind::Empty),
            ("abc", InvalidDaysKind::Malformed),
            ("-1", InvalidDaysKind::Malformed),
            ("3--4", InvalidDaysKind::Malformed),
            ("4294967296", InvalidDaysKind::TooLarge),
        ] {
            assert_eq!(
                parse_due_days(text.into()),
                Err(CoreError::SchedOpRefused {
                    refusal: SchedOpRefusal::InvalidDays { kind }
                })
            );
        }
        for reason in [
            ops::Refusal::InvalidDays(ops::InvalidDays::Empty),
            ops::Refusal::HomeDeckFiltered { deck: DeckId(-5) },
            ops::Refusal::InFilteredDeck { card: CardId(8) },
            ops::Refusal::NotFiltered { deck: DeckId(91) },
            ops::Refusal::PresetUnsupported {
                preset_ids: vec![meguri_model::ids::DeckConfigId(17), meguri_model::ids::DeckConfigId(91)],
            },
        ] {
            let engine = meguri_engine::EngineError::SchedOp(reason);
            assert_eq!(
                meguri_engine::EngineError::from(CoreError::from(engine.clone())),
                engine
            );
        }
    }
    #[test]
    fn operation_dtos_keep_order_targets_and_unmodified_days_text() {
        let request: ops::Request = SchedOperation::SetDue {
            cards: vec![9, -1, 2],
            days: "5-2!".into(),
            context: Some(SchedContext::Browser),
        }
        .into();
        assert!(
            matches!(request,ops::Request::SetDue { cards,days,context:Some(ops::Context::Browser) } if cards==vec![CardId(9),CardId(-1),CardId(2)] && days=="5-2!")
        );
        let request: ops::Request = SchedOperation::Bury {
            targets: SchedTargets::Notes { ids: vec![8, 3] },
        }
        .into();
        assert!(matches!(request,ops::Request::Bury(ops::Targets::Notes(ids)) if ids==vec![NoteId(8),NoteId(3)]));
        let result = SchedMutation {
            summary: MutationSummary {
                seq: 17,
                generation: 3,
                undo_name: Some("Bury".into()),
                changes: (&meguri_model::changes::ChangeSet::empty(meguri_model::changes::OpSource::UserUndoable))
                    .into(),
                past_cancel_point: false,
            },
            count: 4,
            head_diff: Some(crate::review_session::tests::sample_diff().into()),
        };
        assert_eq!(result.head_diff.unwrap().removed, vec![-70, 71]);
        assert_eq!(result.count, 4);
    }
}
