//! Pending postimages are distinct from the queue's presentation-only due moves.
use super::{QueueAnswer, Requeue, SessionQueue};
use meguri_model::{card::Card, ids::CardId};
use std::collections::BTreeMap;

#[derive(Clone, Debug)]
pub struct AcceptedAnswer {
    pub sequence: u64,
    pub postimage: Card,
    pub queue: QueueAnswer,
    pub before: Option<Card>,
}
#[derive(Clone, Debug, Default)]
pub struct Overlay {
    rows: BTreeMap<CardId, Card>,
    pending: Vec<AcceptedAnswer>,
}
impl Overlay {
    pub fn rows(&self) -> impl Iterator<Item = &Card> {
        self.rows.values()
    }
    pub fn row(&self, id: CardId) -> Option<&Card> {
        self.rows.get(&id)
    }
    pub fn predecessor(&self, id: CardId) -> Option<u64> {
        self.pending
            .iter()
            .rev()
            .find(|a| a.postimage.id == id)
            .map(|a| a.sequence)
    }
    pub fn pending(&self) -> &[AcceptedAnswer] {
        &self.pending
    }
    #[allow(clippy::too_many_arguments)] // Selected state distinguishes finished preview from home learning.
    pub fn accept(
        &mut self,
        queue: &mut SessionQueue,
        sequence: u64,
        postimage: Card,
        chosen: crate::states::engine::State,
        buried: &[CardId],
        now: i64,
        boundary: i64,
    ) -> Result<QueueAnswer, super::NotAtHead> {
        let record = queue.answer(postimage.id, Requeue::from_answer(&postimage, chosen, boundary), buried, now)?;
        let before = self.rows.insert(postimage.id, postimage.clone());
        self.pending.push(AcceptedAnswer {
            sequence,
            postimage,
            queue: record.clone(),
            before,
        });
        Ok(record)
    }
    /// A committed prefix ceases to shadow storage only when no later event
    /// depends on its optimistic postimage.
    pub fn committed(&mut self, sequence: u64) {
        if let Some(at) = self.pending.iter().position(|a| a.sequence == sequence) {
            let answer = self.pending.remove(at);
            if !self.pending.iter().any(|a| a.postimage.id == answer.postimage.id) {
                self.rows.remove(&answer.postimage.id);
            }
        }
    }
    pub fn rollback_from(&mut self, sequence: u64, queue: &mut SessionQueue) -> Vec<AcceptedAnswer> {
        let Some(at) = self.pending.iter().position(|a| a.sequence == sequence) else {
            return Vec::new();
        };
        let dropped = self.pending.split_off(at);
        for answer in dropped.iter().rev() {
            let _ = queue.undo(&answer.queue);
            if let Some(before) = &answer.before {
                self.rows.insert(answer.postimage.id, before.clone());
            } else {
                self.rows.remove(&answer.postimage.id);
            }
        }
        self.rows
            .retain(|id, _| self.pending.iter().any(|a| a.postimage.id == *id));
        dropped
    }
}
