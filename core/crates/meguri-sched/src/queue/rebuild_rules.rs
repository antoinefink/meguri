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
pub fn classify(changes: &ChangeSet, images: &[CardImages<'_>], context: Context) -> Action {
    if context.day_changed
        || changes.whole_collection
        || changes.source == OpSource::SyncIncoming
        || context.built.generation != context.commit.generation
    {
        return Action::Rebuild;
    }
    if context.owned == Some(context.commit) {
        return Action::None;
    }
    for change in &changes.cards {
        let flag_only = change.kind == ChangeKind::Updated
            && images
                .iter()
                .find(|r| r.id == change.id)
                .is_some_and(|r| match (r.before, r.after) {
                    (Some(a), Some(b)) if a.len() == 18 && b.len() == 18 => a
                        .iter()
                        .zip(b)
                        .enumerate()
                        .all(|(i, (a, b))| matches!(i, 4 | 5 | 16) || a == b),
                    _ => false,
                });
        if !flag_only {
            return Action::Rebuild;
        }
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
        return Action::Rebuild;
    }
    if !changes.notes.is_empty() || !changes.notetypes.is_empty() {
        Action::Patch
    } else {
        Action::None
    }
}
