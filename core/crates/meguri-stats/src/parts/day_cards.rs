//! The cards behind one day (`stats/history-reads` H13), for Show in
//! Browse: run on a click, not with the page, on the query reader with a
//! new snapshot and the G1 context of the result on screen, so a commit
//! since that result never refuses it.

use std::collections::BTreeSet;
use std::sync::Arc;

use meguri_engine::{CancelToken, Collection, DerivedHost};
use meguri_storage::ReaderRole;

use super::{day_ids, page_day};
use crate::graphs::records::retention::outcome;
use crate::graphs::records::reviews::counted;
use crate::graphs::rows::StatsRow;
use crate::graphs::{Columns, Context, GraphResult, Scope, ScopeError, StatsCaches, StatsError, Unreadable};

/// Which rows of the day name a card (H13's table).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DayKind {
    /// The rows `stats/graphs-revlog` V2 counts (Reviews Over Time).
    Reviewed,
    /// `stats/activity-heatmap` AH2's rows (Activity): `ease` ≥ 1.
    Answered,
    /// H4.1's rows answered Again (Forgotten).
    Forgotten,
}

impl DayKind {
    fn counts(self, row: &StatsRow) -> bool {
        match self {
            DayKind::Reviewed => counted(row),
            DayKind::Answered => row.ease >= 1,
            DayKind::Forgotten => outcome(row).is_some_and(|result| !result.passed),
        }
    }
}

/// One read of H13.
#[derive(Clone, Debug)]
pub struct DayCardsRequest {
    /// The page scope of the result on screen (Collection or Deck).
    pub scope: Scope,
    /// G1's context of the result on screen, from its context token
    /// (G8.3): its `N` places the day.
    pub context: Context,
    /// H1's day: 0 today, `−k` `k` days ago.
    pub day: i32,
    pub kind: DayKind,
    pub cancel: Arc<CancelToken>,
    /// [`Columns::Snapshot`] reads the snapshot itself, never the caches.
    pub columns: Columns,
}

/// What H13 gives.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DayCards {
    /// The existing cards of the scope with a row of the kind on the day,
    /// in ascending id order; never empty.
    Cards(Vec<i64>),
    /// No such card (H13's `none`), also for a day after today.
    NoCards,
    /// A Search scope (H1, H18).
    UnsupportedScope,
    /// G8.2's scope error: a deck id that names no deck now.
    ScopeError(ScopeError),
    /// An unreadable card of the scope, or row of it on the day (G4): its
    /// kind cannot be read, so the list would not be whole.
    UnreadableRow(Unreadable),
    Cancelled,
}

/// H13: the ids of the existing cards of `request.scope` that have a row
/// of `request.kind` on `request.day`, as the collection is now. Call it
/// off the main thread (R1291). Writes nothing (H17).
pub fn stats_day_cards(
    caches: &StatsCaches,
    collection: &Collection,
    derived: &DerivedHost,
    request: &DayCardsRequest,
) -> Result<DayCards, StatsError> {
    let cancel = &request.cancel;
    let scope = request.scope.effective();
    if matches!(scope, Scope::Search(_)) {
        return Ok(DayCards::UnsupportedScope);
    }
    if cancel.is_cancelled() {
        return Ok(DayCards::Cancelled);
    }
    let next_day_at = request.context.next_day_at;
    let Some((start, end)) = day_ids(request.day, next_day_at) else {
        return Ok(DayCards::NoCards);
    };
    // The derived writer applies every commit sent to it before the
    // snapshot's first read of the store (D8), so the caches can serve it.
    if request.columns == Columns::Cached {
        derived.flush()?;
    }
    let lease = collection.reader(ReaderRole::Query)?;
    let snapshot = lease.snapshot()?;
    let resolved = caches.resolve_ids(
        derived,
        &snapshot,
        &request.context,
        scope,
        start,
        end,
        request.columns,
        cancel,
    )?;
    let data = match resolved {
        Ok(data) => data,
        Err(GraphResult::ScopeError(error)) => return Ok(DayCards::ScopeError(error)),
        Err(GraphResult::UnreadableRow(bad)) => return Ok(DayCards::UnreadableRow(bad)),
        Err(GraphResult::Cancelled) => return Ok(DayCards::Cancelled),
        // A resolution gives no other result.
        Err(GraphResult::ContextChanged | GraphResult::Data(_)) => {
            return Err(StatsError::Engine(meguri_engine::EngineError::Internal {
                message: "a day's cards resolved to an unexpected result".into(),
            }));
        }
    };
    let mut cards = BTreeSet::new();
    for scoped in data.rows() {
        // Deleted cards' rows name no card to list.
        let Some(card) = scoped.card else { continue };
        if request.kind.counts(&scoped.row) && page_day(scoped.row.id, next_day_at) == request.day {
            cards.insert(card);
        }
    }
    if cancel.is_cancelled() {
        return Ok(DayCards::Cancelled);
    }
    Ok(if cards.is_empty() { DayCards::NoCards } else { DayCards::Cards(cards.into_iter().collect()) })
}
