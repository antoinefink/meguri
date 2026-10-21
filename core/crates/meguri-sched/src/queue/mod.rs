//! A complete, immutable build value. Mutable answer/session state is U43's.
pub mod bury_gather;
pub mod gather;
pub mod learning;
pub mod limits;
pub mod mix;
pub mod order;
pub mod rebuild_rules;

use crate::model::sched_config::SchedConfig;
use meguri_model::{
    deck::Deck,
    ids::{CardId, DeckConfigId, DeckId},
    proto::{deck::DeckKind, deck_config::DeckConfigMessage},
    time::EpochSecs,
};
use meguri_storage::{ReadRows, StorageError};
use meguri_text::fold::compare_folded;
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Counts {
    pub new: u32,
    pub learning: u32,
    pub review: u32,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    New,
    Review,
    InterdayLearning,
    Learning,
}
impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Self::New => "new",
            Self::Review => "review",
            _ => "learning",
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub id: CardId,
    pub kind: Kind,
    pub mtime: EpochSecs,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LearningEntry {
    pub id: CardId,
    pub due: i64,
    pub reps: u32,
    pub mtime: EpochSecs,
}
#[derive(Clone, Debug)]
pub struct Queue {
    pub deck: DeckId,
    pub main: Vec<Entry>,
    pub learning: Vec<LearningEntry>,
    pub learning_cutoff: i64,
    pub learn_ahead: u32,
    pub counts: Counts,
    pub remaining_limits: limits::LimitTree,
    pub today: u32,
    pub next_day_at: i64,
    pub fsrs: bool,
    pub review_limit_reached: bool,
    pub new_limit_reached: bool,
}
impl Queue {
    pub fn presentation(&self) -> Vec<Entry> {
        let mut out = Vec::with_capacity(self.main.len() + self.learning.len());
        let entry = |c: &LearningEntry| Entry {
            id: c.id,
            mtime: c.mtime,
            kind: Kind::Learning,
        };
        out.extend(
            self.learning
                .iter()
                .filter(|c| c.due <= self.learning_cutoff)
                .map(entry),
        );
        out.extend(self.main.iter().cloned());
        let end = self.learning_cutoff + i64::from(self.learn_ahead);
        out.extend(
            self.learning
                .iter()
                .filter(|c| c.due > self.learning_cutoff && c.due <= end)
                .map(entry),
        );
        out
    }
}

/// Metadata read once from the same snapshot as all card inputs. Deck names
/// use the same folded full-name ordering and component hierarchy as U28.
pub struct Environment {
    pub decks: Vec<Deck>,
    pub parents: Vec<Option<usize>>,
    pub attached: Vec<bool>,
    pub presets: BTreeMap<DeckConfigId, DeckConfigMessage>,
    pub fallback: DeckConfigMessage,
    pub today: u32,
    pub now: i64,
    pub next_day_at: i64,
    pub ahead: u32,
    pub ignore_review: bool,
    pub all_parents: bool,
    pub fsrs: bool,
    pub current: DeckId,
}
impl Environment {
    pub fn read(rows: &impl ReadRows, today: u32, now: i64, next_day_at: i64) -> Result<Self, StorageError> {
        let config = SchedConfig::new(rows);
        let mut decks = rows.deck_rows()?;
        decks.sort_by(|a, b| compare_folded(&a.name, &b.name));
        let parents = decks
            .iter()
            .map(|d| {
                d.name
                    .rsplit_once('\u{1f}')
                    .and_then(|(p, _)| decks.iter().position(|a| compare_folded(&a.name, p).is_eq()))
            })
            .collect::<Vec<_>>();
        let attached = decks
            .iter()
            .enumerate()
            .map(|(i, _)| {
                let mut cursor = i;
                loop {
                    if let Some(p) = parents[cursor] {
                        cursor = p;
                    } else {
                        break !decks[cursor].name.contains('\u{1f}');
                    }
                }
            })
            .collect();
        let presets = rows
            .deck_config_rows()?
            .into_iter()
            .map(|p| (p.id, p.config.for_use()))
            .collect();
        let requested = DeckId(rows.config_value("curDeck")?.and_then(|v| v.as_i64()).unwrap_or(1));
        let current = if decks.iter().any(|d| d.id == requested) {
            requested
        } else {
            DeckId(1)
        };
        Ok(Self {
            decks,
            parents,
            attached,
            presets,
            fallback: DeckConfigMessage::builtin_defaults(),
            today,
            now,
            next_day_at,
            ahead: config.collapse_time()?,
            ignore_review: config.new_cards_ignore_review_limit()?,
            all_parents: config.apply_all_parent_limits()?,
            fsrs: config.fsrs()?,
            current,
        })
    }
    pub fn index(&self, id: DeckId) -> Option<usize> {
        self.decks.iter().position(|d| d.id == id)
    }
    pub fn filtered(&self, i: usize) -> bool {
        matches!(self.decks[i].kind.kind, DeckKind::Filtered(_))
    }
    pub fn preset(&self, i: usize) -> Option<&DeckConfigMessage> {
        match &self.decks[i].kind.kind {
            DeckKind::Normal(n) => self.presets.get(&DeckConfigId(n.config_id)),
            _ => None,
        }
    }
    pub fn subtree(&self, id: DeckId, normal_only: bool) -> Vec<DeckId> {
        let Some(root) = self.index(id) else { return Vec::new() };
        self.decks
            .iter()
            .enumerate()
            .filter(|(i, _)| {
                if !self.attached[*i] || (normal_only && self.filtered(*i)) {
                    return false;
                }
                let mut cursor = Some(*i);
                while let Some(j) = cursor {
                    if j == root {
                        return true;
                    }
                    cursor = self.parents[j];
                }
                false
            })
            .map(|(_, d)| d.id)
            .collect()
    }
    pub fn bury_preset(&self, card: &meguri_model::card::Card) -> &DeckConfigMessage {
        let home = if card.original_deck_id.0 != 0 {
            card.original_deck_id
        } else {
            card.deck_id
        };
        self.index(home).and_then(|i| self.preset(i)).unwrap_or(&self.fallback)
    }
}

pub fn build(rows: &impl ReadRows, env: &Environment, deck: DeckId) -> Result<Queue, StorageError> {
    gather::build(rows, env, deck)
}
