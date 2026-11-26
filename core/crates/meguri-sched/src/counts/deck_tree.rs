//! C1-C6. One grouped storage read supplies every node; sibling burying
//! never enters this computation.
use crate::queue::{Counts, Environment, limits::Remaining};
use meguri_model::ids::DeckId;
use meguri_storage::queries::sched_queue::{DeckAggregate, Totals};
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Node {
    pub id: DeckId,
    pub name: String,
    pub filtered: bool,
    pub study_collapsed: bool,
    pub browser_collapsed: bool,
    pub counts: Counts,
    pub children: Vec<Node>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeckTree {
    pub nodes: Vec<Node>,
    pub next_change_at: i64,
}
impl DeckTree {
    pub fn counts(&self, id: DeckId) -> Counts {
        fn find(nodes: &[Node], id: DeckId) -> Option<Counts> {
            nodes.iter().find_map(|n| {
                if n.id == id {
                    Some(n.counts)
                } else {
                    find(&n.children, id)
                }
            })
        }
        find(&self.nodes, id).unwrap_or_default()
    }
}
fn add(a: &mut Totals, b: Totals) {
    a.new += b.new;
    a.review += b.review;
    a.interday += b.interday;
    a.intraday += b.intraday;
}
fn cap(mut t: Totals, r: Remaining) -> Totals {
    t.interday = t.interday.min(r.review as u32);
    let left = r.review as u32 - t.interday;
    t.review = t.review.min(left);
    t.new = t.new.min(r.new as u32);
    if r.capped_by_reviews {
        t.new = t.new.min(left - t.review);
    }
    t
}
pub fn build(env: &Environment, groups: &[DeckAggregate], tomorrow: bool) -> DeckTree {
    let groups = groups.iter().map(|g| (g.deck, g)).collect::<BTreeMap<_, _>>();
    let mut effective = Vec::<Remaining>::with_capacity(env.decks.len());
    for (i, _) in env.decks.iter().enumerate() {
        let mut r = Remaining::for_deck(env, i, tomorrow);
        if env.all_parents && !env.filtered(i) {
            let parent = env.parents[i].map(|p| effective[p]).unwrap_or_else(Remaining::uncapped);
            r.new = r.new.min(parent.new);
            r.review = r.review.min(parent.review);
        }
        effective.push(r);
    }
    // U28's Default visibility uses current did membership, including cards
    // outside the due queues, and never odid membership.
    let other_root = env
        .decks
        .iter()
        .enumerate()
        .any(|(i, d)| d.id != DeckId(1) && env.parents[i].is_none() && env.attached[i]);
    let hide_default = other_root
        && groups.get(&DeckId(1)).is_none_or(|g| g.cards == 0)
        && env
            .index(DeckId(1))
            .is_some_and(|i| env.parents[i].is_none() && !env.parents.contains(&Some(i)));
    fn children(
        env: &Environment,
        parent: Option<usize>,
        groups: &BTreeMap<DeckId, &DeckAggregate>,
        effective: &[Remaining],
        tomorrow: bool,
        hide_default: bool,
    ) -> Vec<(Node, Totals)> {
        env.decks
            .iter()
            .enumerate()
            .filter(|(i, d)| env.attached[*i] && env.parents[*i] == parent && !(hide_default && d.id == DeckId(1)))
            .map(|(i, d)| {
                let nested = children(env, Some(i), groups, effective, tomorrow, hide_default);
                let mut totals = groups
                    .get(&d.id)
                    .map_or(Totals::default(), |g| if tomorrow { g.tomorrow } else { g.today });
                for (_, t) in &nested {
                    add(&mut totals, *t);
                }
                let totals = if env.filtered(i) { totals } else { cap(totals, effective[i]) };
                (
                    Node {
                        id: d.id,
                        name: d.name.rsplit('\u{1f}').next().unwrap_or_default().into(),
                        filtered: env.filtered(i),
                        study_collapsed: d.common.study_collapsed,
                        browser_collapsed: d.common.browser_collapsed,
                        counts: Counts {
                            new: totals.new,
                            review: totals.review,
                            learning: totals.intraday + totals.interday,
                        },
                        children: nested.into_iter().map(|(n, _)| n).collect(),
                    },
                    totals,
                )
            })
            .collect()
    }
    let next_change_at = groups
        .values()
        .filter(|g| env.index(g.deck).is_some_and(|i| env.attached[i]))
        .filter_map(|g| g.next_change_at)
        .filter(|t| *t > env.now)
        .min()
        .unwrap_or(env.next_day_at.max(env.now + 1))
        .min(env.next_day_at.max(env.now + 1));
    DeckTree {
        nodes: children(env, None, &groups, &effective, tomorrow, hide_default)
            .into_iter()
            .map(|(n, _)| n)
            .collect(),
        next_change_at,
    }
}
