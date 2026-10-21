//! Read-only E2 admission onto the real queue and U44 answer path.
//! This owner deliberately has no shown-card state or selection effects.
use crate::{
    Collection, Direction, EngineError, OpResult,
    sched::{
        answer::{self, AnswerCache, AnswerRequest, AnswerResult},
        queue::QueueProvider,
    },
};
use meguri_model::ids::{CardId, DeckId};
use meguri_sched::{
    queue::{Counts, Entry},
    session::{QueueAnswer, Requeue, SessionQueue},
};
use std::sync::{Arc, Mutex, Weak};

#[derive(Debug)]
pub(crate) struct HeldState {
    pub queue: SessionQueue,
    pub rebuild: bool,
}
#[derive(Clone, Debug)]
pub(crate) struct QueueRestore {
    pub owner: Weak<Mutex<HeldState>>,
    pub record: QueueAnswer,
}
impl QueueRestore {
    pub(crate) fn prepare(
        owner: &Arc<Mutex<HeldState>>,
        card: &meguri_model::card::Card,
        buried: &[CardId],
        now: i64,
        boundary: i64,
    ) -> Result<(Self, SessionQueue), EngineError> {
        let held = owner.lock().unwrap();
        let mut next = held.queue.clone();
        let record = next
            .answer(card.id, Requeue::from_card(card, boundary), buried, now)
            .map_err(|_| EngineError::InvalidArgument {
                message: "NotAtHead".into(),
            })?;
        Ok((
            Self {
                owner: Arc::downgrade(owner),
                record,
            },
            next,
        ))
    }
    pub(crate) fn commit(&self, next: SessionQueue) {
        if let Some(owner) = self.owner.upgrade() {
            let mut held = owner.lock().unwrap();
            if held.queue.build() == self.record.build {
                held.queue = next;
            } else {
                held.rebuild = true;
            }
        }
    }
    pub(crate) fn restore(&self, direction: Direction) {
        if let Some(owner) = self.owner.upgrade() {
            let mut held = owner.lock().unwrap();
            let result = match direction {
                Direction::Undo => held.queue.undo(&self.record),
                Direction::Redo => held.queue.redo(&self.record),
            };
            if result.is_err() {
                held.rebuild = true;
            }
        }
    }
}

/// This is an engine-held queue, including its real U44 cache and restorer.
/// It is also the compatibility seam available to neutral scenario adapters.
#[derive(Debug)]
pub struct HeldQueue {
    collection: Collection,
    deck: Option<DeckId>,
    state: Arc<Mutex<HeldState>>,
    cache: Arc<AnswerCache>,
}
impl HeldQueue {
    pub fn build(collection: &Collection, deck: Option<DeckId>) -> Result<Self, EngineError> {
        loop {
            let seq = collection.last_seq();
            let cache = AnswerCache::build_scenario_held(collection)?;
            let queue = QueueProvider::default().compatibility_queue(collection, deck)?;
            if collection.last_seq() != seq || !cache.is_current(collection) {
                continue;
            }
            return Ok(Self {
                collection: collection.clone(),
                deck,
                state: Arc::new(Mutex::new(HeldState {
                    queue: SessionQueue::new(queue, cache.identity().0),
                    rebuild: false,
                })),
                cache,
            });
        }
    }
    pub fn cache(&self) -> Arc<AnswerCache> {
        self.cache.clone()
    }
    pub fn bound_to(&self, collection: &Collection) -> bool {
        self.collection.generation() == collection.generation() && Arc::ptr_eq(&self.collection.core, &collection.core)
    }
    pub fn invalidate(&self) {
        self.cache.invalidate();
        self.state.lock().unwrap().rebuild = true;
    }
    pub fn refresh(&mut self) -> Result<(), EngineError> {
        let replacement = Self::build(&self.collection, self.deck)?;
        let queue = replacement.state.lock().unwrap().queue.clone();
        let mut state = self.state.lock().unwrap();
        state.queue = queue;
        state.rebuild = false;
        self.cache = replacement.cache;
        Ok(())
    }
    pub fn is_current(&self) -> bool {
        self.collection.is_current() && self.cache.is_current(&self.collection) && !self.state.lock().unwrap().rebuild
    }
    pub fn boundary(&self) -> i64 {
        self.cache.next_day_at()
    }
    pub fn deck(&self) -> Option<DeckId> {
        self.deck
    }
    pub fn read(&self, now: i64) -> (Vec<Entry>, Counts) {
        let mut state = self.state.lock().unwrap();
        let entries = state.queue.read(now, usize::MAX);
        (entries, state.queue.value().counts)
    }
    pub fn answer(&self, mut request: AnswerRequest) -> Result<OpResult<AnswerResult>, EngineError> {
        {
            let state = self.state.lock().unwrap();
            if !state.queue.permits(request.card) {
                return Err(EngineError::InvalidArgument {
                    message: "NotAtHead".into(),
                });
            }
            request.from_queue = state.queue.kind(request.card);
            request.learning_cutoff = Some(state.queue.value().learning_cutoff);
        }
        request.cache = Some(self.cache.clone());
        answer::answer_held(&self.collection, request, self.state.clone())
    }
}
