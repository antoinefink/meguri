//! Browse's card actions other than Change Deck (`browser/workspace` V8.1,
//! `browser/bulk-actions` B1, B10, B16) as bulk actions
//! (`browser/bulk-framework` K7).
//!
//! - **Flag** sets the chosen flag on every target card, No Flag (0) clears
//!   it (V8.1: Browse's Flag sets, it does not toggle): `tags/flags-and-marked`
//!   F2, "Set Flag", the card's `mod` kept.
//! - **Suspend** and **Bury** are `sched/manual-ops` M3 and M2 on every
//!   target card; **Unsuspend** and **Unbury** are M4 on the target cards
//!   that are suspended, or buried, when the commit reads them, and on no
//!   other (V8.1): a buried card stays buried through Unsuspend and a
//!   suspended one suspended through Unbury. Both are one "Unbury/Unsuspend"
//!   step (B16).
//! - **Set Due Date** and **Forget** are B10's, in the browser context: the
//!   text as typed is stored in `setDueBrowser`, the two choices in
//!   `restorePositionBrowser` and `resetCountsBrowser`, inside the action's
//!   own step (`browser/config-keys` K6.6).
//!
//! The scheduling actions run `sched::ops`' operation in the commit, which
//! reads every target card from its live row (K3: answering writes cards and
//! the review log, so they are re-derived, never covered by a table
//! precondition). Like `sched::ops::run` they refuse while a review session
//! is active (`SessionActive`) unless the review service admitted them
//! ([`run_admitted`]): it drains the answers it accepted first. Flag
//! re-reads its cards too; a caller with a review service runs it admitted
//! as well, so a held answer is written before the card row changes.

use std::sync::Arc;

use meguri_model::ids::CardId;
use meguri_storage::{ReadRows, ReaderRole, StampMode};

use super::{BulkCounts, BulkJob, BulkOutcome, CardTarget, check_ids, op_spec};
use crate::cancel::CancelToken;
use crate::collection::Collection;
use crate::error::EngineError;
use crate::executor::op::{OpContext, OpResult};
use crate::executor::plan_commit::{Plan, PlanContext, plan_and_commit_with};
use crate::lifecycle::Engine;
use crate::progress::ProgressSink;
use crate::sched::ops::{self, Context, Request, Targets};
use crate::undo::names::{BURY, RESET_CARD, SET_DUE_DATE, SET_FLAG, SUSPEND, UNBURY_UNSUSPEND};

/// A card action of V8.1 besides Change Deck.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CardAction {
    /// Flag 1 to 7, or 0 for No Flag (F2). A value above 7 is refused with
    /// `InvalidArgument` (F2's `invalid_flag`).
    SetFlag(u8),
    Suspend,
    /// M4 on the suspended target cards only.
    Unsuspend,
    Bury,
    /// M4 on the buried target cards only (by the user or the scheduler).
    Unbury,
    /// `sched/set-due-date` with the days text as typed.
    SetDueDate { days: String },
    /// `sched/manual-ops` M6 (Forget) with its two choices.
    Forget { restore_position: bool, reset_counts: bool },
}

impl CardAction {
    /// The engine op's name (`engine/op-model` O1).
    fn op_name(&self) -> &'static str {
        match self {
            CardAction::SetFlag(_) => "set_flag",
            CardAction::Suspend => "suspend_cards",
            CardAction::Unsuspend => "unsuspend_cards",
            CardAction::Bury => "bury_cards",
            CardAction::Unbury => "unbury_cards",
            CardAction::SetDueDate { .. } => "set_due_date",
            CardAction::Forget { .. } => "forget_cards",
        }
    }

    /// The undo step (B16), fixed before the commit reads anything (K5).
    fn step(&self) -> &'static str {
        match self {
            CardAction::SetFlag(_) => SET_FLAG,
            CardAction::Suspend => SUSPEND,
            CardAction::Unsuspend | CardAction::Unbury => UNBURY_UNSUSPEND,
            CardAction::Bury => BURY,
            CardAction::SetDueDate { .. } => SET_DUE_DATE,
            CardAction::Forget { .. } => RESET_CARD,
        }
    }

    /// Whether the action writes scheduling: every one but Flag. Those run
    /// admitted by the review service while one can be active.
    pub fn writes_scheduling(&self) -> bool {
        !matches!(self, CardAction::SetFlag(_))
    }

    /// The refusals that need no rows: a flag above 7 (F2), days text that
    /// does not parse (`sched/set-due-date` S1, `SchedOp(InvalidDays)`).
    fn check(&self) -> Result<(), EngineError> {
        match self {
            CardAction::SetFlag(flag) => {
                crate::flags::check_flag(u64::from(*flag)).map_err(|error| EngineError::InvalidArgument {
                    message: error.to_string(),
                })?;
            }
            CardAction::SetDueDate { days } => {
                ops::Days::parse(days).map_err(|error| EngineError::SchedOp(ops::Refusal::InvalidDays(error)))?;
            }
            _ => {}
        }
        Ok(())
    }

    fn stamp(&self) -> StampMode {
        match self {
            // F2: a flag change keeps the card's `mod`.
            CardAction::SetFlag(_) => StampMode::KeepMtime,
            // `sched::ops` stamps each card it writes itself.
            _ => StampMode::CallerSupplied,
        }
    }
}

/// What the compute phase hands the commit: only the ids (K7).
#[derive(Debug)]
struct Planned {
    cards: Vec<CardId>,
}

/// The compute phase: nothing to read ahead, since the commit re-derives
/// every card from its live row (K3); the cancel point and the progress
/// of the `Planning` phase.
fn plan(context: &PlanContext<'_>, cards: &[CardId]) -> Result<Plan<Planned>, EngineError> {
    context.check_cancel()?;
    if let Some(progress) = context.progress() {
        let total = cards.len() as u64;
        progress.report(total, Some(total));
    }
    Ok(Plan {
        value: Planned { cards: cards.to_vec() },
        preconditions: Vec::new(),
    })
}

/// The cards of `ids` whose live queue `keep` accepts, in list order.
fn live_subset(
    context: &mut OpContext<'_, '_>,
    ids: &[CardId],
    keep: impl Fn(meguri_model::card::CardQueue) -> bool,
) -> Result<Vec<CardId>, EngineError> {
    let mut kept = Vec::new();
    for id in ids {
        if let Some(card) = context.scope().card(*id)?
            && keep(card.queue)
        {
            kept.push(*id);
        }
    }
    Ok(kept)
}

/// The write phase.
fn commit(
    context: &mut OpContext<'_, '_>,
    bound: &Collection,
    action: &CardAction,
    day: Option<&crate::DayReport>,
    require_idle: bool,
    planned: Planned,
) -> Result<BulkCounts, EngineError> {
    use meguri_model::card::CardQueue;
    let considered = planned.cards.len() as u64;
    let cards = planned.cards;
    let request = match action {
        CardAction::SetFlag(flag) => {
            let written = crate::flags::set_flag_in(context.scope(), &cards, *flag)?;
            return Ok(BulkCounts {
                affected: written as u64,
                considered,
            });
        }
        CardAction::Suspend => Request::Suspend(Targets::Cards(cards)),
        CardAction::Bury => Request::Bury(Targets::Cards(cards)),
        CardAction::Unsuspend => Request::Unsuspend(live_subset(context, &cards, |queue| {
            queue == CardQueue::Suspended
        })?),
        CardAction::Unbury => Request::Unsuspend(live_subset(context, &cards, |queue| {
            matches!(queue, CardQueue::BuriedByUser | CardQueue::BuriedByScheduler)
        })?),
        CardAction::SetDueDate { days } => Request::SetDue {
            cards,
            days: days.clone(),
            context: Some(Context::Browser),
        },
        CardAction::Forget {
            restore_position,
            reset_counts,
        } => Request::Reset {
            cards,
            restore_position: *restore_position,
            reset_counts: *reset_counts,
            context: Some(Context::Browser),
        },
    };
    let day = day.ok_or_else(|| EngineError::internal("a scheduling action runs with the day it read"))?;
    let done = ops::apply_in(context, bound, &request, day, require_idle)?;
    Ok(BulkCounts {
        affected: done.count as u64,
        considered,
    })
}

fn run_with(
    collection: &Collection,
    target: &CardTarget,
    action: &CardAction,
    cancel: Arc<CancelToken>,
    progress: ProgressSink,
    require_idle: bool,
) -> Result<OpResult<BulkCounts>, EngineError> {
    cancel.check()?;
    let cards = target.resolve(collection)?;
    check_ids(&cards, "card")?;
    action.check()?;
    if require_idle && action.writes_scheduling() && collection.core.exec.active_session.lock().unwrap().is_some() {
        return Err(EngineError::SessionActive);
    }
    // The day, before the op: never inside it (`sched/day-clock` D8).
    let day = if action.writes_scheduling() {
        Some(collection.check_rollover()?)
    } else {
        None
    };
    let (name, step, stamp) = (action.op_name(), action.step(), action.stamp());
    let action = Arc::new(action.clone());
    let bound = collection.clone();
    let (cancel_plan, progress_plan) = (Arc::clone(&cancel), progress.clone());
    plan_and_commit_with(
        collection,
        ReaderRole::Maintenance,
        op_spec(name, step, 0, &cancel, &progress).stamp(stamp),
        move |context| plan(context, &cards),
        move |_, planned| op_spec(name, step, planned.cards.len(), &cancel_plan, &progress_plan).stamp(stamp),
        move |context, planned| {
            let require_idle = require_idle && action.writes_scheduling();
            commit(context, &bound, &action, day.as_ref(), require_idle, planned)
        },
    )
}

/// The action on `target`, blocking: a compute phase on a maintenance
/// snapshot, then one write transaction and one undo step (B16). A
/// scheduling action refuses while a review session is active
/// (`SessionActive`).
pub fn run(
    collection: &Collection,
    target: &CardTarget,
    action: &CardAction,
    cancel: Arc<CancelToken>,
    progress: ProgressSink,
) -> Result<OpResult<BulkCounts>, EngineError> {
    run_with(collection, target, action, cancel, progress, true)
}

/// [`run`] for a caller the review service admitted
/// (`ReviewService::admit_card_action`): an active session does not refuse.
pub fn run_admitted(
    collection: &Collection,
    target: &CardTarget,
    action: &CardAction,
    cancel: Arc<CancelToken>,
    progress: ProgressSink,
) -> Result<OpResult<BulkCounts>, EngineError> {
    run_with(collection, target, action, cancel, progress, false)
}

/// [`run`] as a job (`browser/bulk-framework` K2).
pub fn start(engine: &Engine, collection: Collection, target: CardTarget, action: CardAction) -> Result<BulkJob, EngineError> {
    super::start(engine, move |context| {
        run(
            &collection,
            &target,
            &action,
            Arc::clone(&context.cancel),
            context.progress.clone(),
        )
        .map(BulkOutcome::of)
    })
}
