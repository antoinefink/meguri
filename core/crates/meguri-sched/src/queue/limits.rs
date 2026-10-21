use super::{Environment, Kind};
use crate::model::deck_sched::DeckSched;
use meguri_model::{ids::DeckId, proto::deck::DeckKind};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Remaining {
    pub new: i32,
    pub review: i32,
    pub capped_by_reviews: bool,
}
impl Remaining {
    pub fn normal(new: u32, review: u32, new_studied: i32, review_studied: i32, ignore: bool) -> Self {
        let mut review = (review as i32).wrapping_sub(review_studied);
        let mut new = (new as i32).wrapping_sub(new_studied);
        if !ignore {
            review = review.wrapping_sub(new_studied);
            new = new.min(review);
        }
        Self {
            new: new.max(0),
            review: review.max(0),
            capped_by_reviews: !ignore,
        }
    }
    pub fn uncapped() -> Self {
        Self {
            new: 9999,
            review: 9999,
            capped_by_reviews: false,
        }
    }
    pub fn for_deck(env: &Environment, i: usize, tomorrow: bool) -> Self {
        let Some(p) = env.preset(i) else {
            return Self::uncapped();
        };
        let d = &env.decks[i];
        let DeckKind::Normal(n) = &d.kind.kind else {
            return Self::uncapped();
        };
        let day = if tomorrow { env.today.wrapping_add(1) } else { env.today };
        let sched = DeckSched::new(Some(n), &d.common);
        let (ns, rs) = if tomorrow { (0, 0) } else { sched.studied(day) };
        Self::normal(
            sched.new_limit(day, p.new_per_day).unwrap(),
            sched.review_limit(day, p.reviews_per_day).unwrap(),
            ns,
            rs,
            env.ignore_review,
        )
    }
    fn available(self, kind: Kind) -> i32 {
        if kind == Kind::New { self.new } else { self.review }
    }
}
#[derive(Clone, Debug)]
pub struct LimitNode {
    pub parent: Option<DeckId>,
    pub remaining: Remaining,
}
pub type LimitTree = BTreeMap<DeckId, LimitNode>;

pub fn tree(env: &Environment, deck: DeckId, selected: &[DeckId]) -> LimitTree {
    let mut out = LimitTree::new();
    for id in selected {
        let mut cursor = env.index(*id);
        while let Some(i) = cursor {
            let d = &env.decks[i];
            if !env.attached[i] {
                break;
            }
            let parent = if d.id == deck && !env.all_parents {
                None
            } else {
                env.parents[i].map(|p| env.decks[p].id)
            };
            out.entry(d.id).or_insert_with(|| LimitNode {
                parent,
                remaining: Remaining::for_deck(env, i, false),
            });
            cursor = parent.and_then(|id| env.index(id));
        }
    }
    out
}
pub fn permits(tree: &LimitTree, deck: DeckId, kind: Kind) -> bool {
    let mut cursor = Some(deck);
    while let Some(id) = cursor {
        let Some(node) = tree.get(&id) else { return false };
        if node.remaining.available(kind) <= 0 {
            return false;
        }
        cursor = node.parent;
    }
    true
}
pub fn take(tree: &mut LimitTree, deck: DeckId, kind: Kind) {
    let mut cursor = Some(deck);
    while let Some(id) = cursor {
        let node = tree.get_mut(&id).expect("permitted path");
        if kind == Kind::New {
            node.remaining.new -= 1;
        } else {
            node.remaining.review -= 1;
            if node.remaining.capped_by_reviews {
                node.remaining.new = node.remaining.new.min(node.remaining.review);
            }
        }
        cursor = node.parent;
    }
}
