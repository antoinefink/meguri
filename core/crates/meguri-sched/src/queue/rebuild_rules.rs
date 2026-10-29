use meguri_model::{
    changes::{ChangeKind, ChangeSet, OpSource},
    ids::CardId,
};
use meguri_storage::Cell;
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Action {
    None,
    Patch,
    Rebuild,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Stamp {
    pub seq: u64,
    pub generation: u64,
}
#[derive(Clone, Copy, Debug)]
pub struct CardImages<'a> {
    pub id: CardId,
    pub before: Option<&'a [Cell]>,
    pub after: Option<&'a [Cell]>,
}
#[derive(Clone, Copy, Debug)]
pub struct Context {
    pub built: Stamp,
    pub commit: Stamp,
    pub owned: Option<Stamp>,
    pub day_changed: bool,
}
/// Compatibility slice adapter. Production passes an on-demand raw-cell proof.
pub fn classify(changes: &ChangeSet, images: &[CardImages<'_>], context: Context) -> Action {
    let result = classify_with(changes, context, |id| {
        Ok::<_, std::convert::Infallible>(
            images
                .iter()
                .find(|r| r.id == id)
                .is_some_and(|r| flag_only(r.before, r.after)),
        )
    });
    match result {
        Ok(action) => action,
        Err(never) => match never {},
    }
}

pub fn flag_only(before: Option<&[Cell]>, after: Option<&[Cell]>) -> bool {
    match (before, after) {
        (Some(a), Some(b)) if a.len() == 18 && b.len() == 18 => a
            .iter()
            .zip(b)
            .enumerate()
            .all(|(i, (a, b))| matches!(i, 4 | 5 | 16) || a == b),
        _ => false,
    }
}

/// Compact-fact exits precede payload access. An unreadable row cannot prove
/// flag-only; callers must invalidate/rebuild on the returned error.
pub fn classify_with<E>(
    changes: &ChangeSet,
    context: Context,
    mut flag: impl FnMut(CardId) -> Result<bool, E>,
) -> Result<Action, E> {
    if context.day_changed
        || changes.whole_collection
        || matches!(
            changes.source,
            OpSource::SyncIncoming | OpSource::CloudIncoming | OpSource::CloudRekey
        )
        || context.built.generation != context.commit.generation
    {
        return Ok(Action::Rebuild);
    }
    if context.owned == Some(context.commit) {
        return Ok(Action::None);
    }
    if !changes.decks.is_empty()
        || !changes.deck_configs.is_empty()
        || changes.config_keys.iter().any(|c| {
            matches!(
                c.key.as_str(),
                "curDeck"
                    | "collapseTime"
                    | "applyAllParentLimits"
                    | "newCardsIgnoreReviewLimit"
                    | "fsrs"
                    | "fsrsShortTermWithStepsEnabled"
                    | "loadBalancerEnabled"
                    | "rollover"
                    | "creationOffset"
                    | "localOffset"
            )
        })
    {
        return Ok(Action::Rebuild);
    }
    for change in &changes.cards {
        if change.kind != ChangeKind::Updated || !flag(change.id)? {
            return Ok(Action::Rebuild);
        }
    }
    if !changes.notes.is_empty() || !changes.notetypes.is_empty() {
        Ok(Action::Patch)
    } else {
        Ok(Action::None)
    }
}
