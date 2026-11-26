//! Held presentation, shared by production review and the E2 compatibility owner.
//! No collection access, clock reads, executor, rendering or event delivery.
pub mod overlay;
pub mod token;

use crate::queue::{Counts, Entry, Kind, LearningEntry, Queue};
use meguri_model::{
    card::{Card, CardQueue},
    ids::CardId,
    time::EpochSecs,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NotAtHead;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RestoreError {
    BuildChanged,
    MissingEntry,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Requeue {
    pub due: i64,
    pub reps: u32,
    pub mtime: EpochSecs,
}
impl Requeue {
    pub fn from_answer(card: &Card, chosen: crate::states::engine::State, boundary: i64) -> Option<Self> {
        // Finishing preview restores the home queue. A home learning timestamp
        // before rollover does not make the card a repeat of this session.
        if matches!(chosen, crate::states::engine::State::Preview { finished: true, .. }) {
            return None;
        }
        (matches!(card.queue, CardQueue::Learning | CardQueue::Preview) && i64::from(card.due) < boundary).then_some(
            Self {
                due: i64::from(card.due),
                reps: card.reps,
                mtime: card.mtime,
            },
        )
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Removed {
    Main(Entry),
    Learning(LearningEntry),
}
impl Removed {
    pub fn id(&self) -> CardId {
        match self {
            Self::Main(e) => e.id,
            Self::Learning(e) => e.id,
        }
    }
    pub fn kind(&self) -> Kind {
        match self {
            Self::Main(e) => e.kind,
            Self::Learning(_) => Kind::Learning,
        }
    }
}
#[derive(Clone, Debug)]
pub struct QueueAnswer {
    pub build: u64,
    pub removed: Removed,
    pub requeued: Option<LearningEntry>,
    pub learning_after_requeue: u32,
    pub cutoff_before_advance: i64,
    // Main-list positions after removal of the answered entry, before sibling removal.
    siblings: Vec<(usize, Entry)>,
}

#[derive(Clone, Debug)]
pub struct SessionQueue {
    value: Queue,
    build: u64,
    current: Option<CardId>,
}
impl SessionQueue {
    pub fn new(value: Queue, build: u64) -> Self {
        Self {
            value,
            build,
            current: None,
        }
    }
    pub fn value(&self) -> &Queue {
        &self.value
    }
    pub fn build(&self) -> u64 {
        self.build
    }
    pub fn current(&self) -> Option<CardId> {
        self.current
    }
    pub fn rebuild(&mut self, value: Queue, build: u64) {
        self.value = value;
        self.build = build;
        self.keep_current();
    }
    pub fn set_current(&mut self, current: Option<CardId>) {
        self.current = current;
        self.keep_current();
    }
    fn keep_current(&mut self) {
        if self.current.is_some_and(|id| !self.presented(id)) {
            self.current = None;
        }
    }
    pub fn presented(&self, id: CardId) -> bool {
        self.value.main.iter().any(|e| e.id == id)
            || self.value.learning.iter().any(|e| e.id == id && e.due <= self.edge())
    }
    pub fn kind(&self, id: CardId) -> Option<Kind> {
        if self.value.learning.iter().any(|e| e.id == id) {
            Some(Kind::Learning)
        } else {
            self.value.main.iter().find(|e| e.id == id).map(|e| e.kind)
        }
    }
    pub fn permits(&self, id: CardId) -> bool {
        self.current == Some(id)
            || self.value.main.first().is_some_and(|e| e.id == id)
            || self.value.learning.iter().any(|e| e.id == id)
    }
    fn edge(&self) -> i64 {
        self.value
            .learning_cutoff
            .saturating_add(i64::from(self.value.learn_ahead))
    }
    fn advance(&mut self, now: i64) {
        let old = self.edge();
        if now >= self.value.learning_cutoff {
            let edge = now.saturating_add(i64::from(self.value.learn_ahead));
            let added = self
                .value
                .learning
                .iter()
                .filter(|e| old < e.due && e.due <= edge)
                .count() as u32;
            self.value.counts.learning = self.value.counts.learning.saturating_add(added);
        }
        self.value.learning_cutoff = now;
    }
    pub fn read(&mut self, now: i64, limit: usize) -> Vec<Entry> {
        if self.value.counts == Counts::default() {
            self.advance(now);
        }
        self.keep_current();
        let learning = |e: &LearningEntry| Entry {
            id: e.id,
            kind: Kind::Learning,
            mtime: e.mtime,
        };
        let order = self
            .value
            .learning
            .iter()
            .filter(|e| e.due <= self.value.learning_cutoff)
            .map(learning)
            .chain(self.value.main.iter().cloned())
            .chain(
                self.value
                    .learning
                    .iter()
                    .filter(|e| self.value.learning_cutoff < e.due && e.due <= self.edge())
                    .map(learning),
            );
        let current = self.current.and_then(|id| {
            self.value
                .main
                .iter()
                .find(|e| e.id == id)
                .cloned()
                .or_else(|| self.value.learning.iter().find(|e| e.id == id).map(learning))
        });
        current
            .into_iter()
            .chain(order.filter(|e| Some(e.id) != self.current))
            .take(limit)
            .collect()
    }
    pub fn waiting_until(&self) -> Option<i64> {
        self.value
            .learning
            .iter()
            .filter(|e| e.due > self.edge())
            .map(|e| e.due.saturating_sub(i64::from(self.value.learn_ahead)))
            .min()
    }
    /// C7: state-boundary candidates are supplied by the U45 head projection.
    pub fn valid_until(&self, now: i64, state_boundaries: impl IntoIterator<Item = i64>) -> i64 {
        std::iter::once(self.value.next_day_at)
            .chain(
                self.value
                    .learning
                    .iter()
                    .flat_map(|e| [e.due, e.due.saturating_sub(i64::from(self.value.learn_ahead))]),
            )
            .chain(state_boundaries)
            .filter(|at| *at > now)
            .min()
            .unwrap_or(self.value.next_day_at)
    }
    fn decrement(&mut self, kind: Kind) {
        let count = match kind {
            Kind::New => &mut self.value.counts.new,
            Kind::Review => &mut self.value.counts.review,
            _ => &mut self.value.counts.learning,
        };
        *count = count.saturating_sub(1);
    }
    fn increment(&mut self, kind: Kind) {
        let count = match kind {
            Kind::New => &mut self.value.counts.new,
            Kind::Review => &mut self.value.counts.review,
            _ => &mut self.value.counts.learning,
        };
        *count = count.saturating_add(1);
    }
    fn remove(&mut self, id: CardId) -> Result<Removed, NotAtHead> {
        if let Some(i) = self.value.learning.iter().position(|e| e.id == id) {
            self.decrement(Kind::Learning);
            return Ok(Removed::Learning(self.value.learning.remove(i)));
        }
        if !self.permits(id) {
            return Err(NotAtHead);
        }
        let i = self.value.main.iter().position(|e| e.id == id).ok_or(NotAtHead)?;
        let entry = self.value.main.remove(i);
        self.decrement(entry.kind);
        Ok(Removed::Main(entry))
    }
    fn insert_learning(&mut self, entry: LearningEntry) {
        // P5 compares attempt status before due time. Scan the whole list:
        // P9 restores at the front, so a larger key need not end the search.
        let incoming = (entry.reps == 0, entry.due);
        let mut position = 0;
        for (index, existing) in self.value.learning.iter().enumerate() {
            let candidate = (existing.reps == 0, existing.due);
            if candidate <= incoming {
                position = index + 1;
            }
        }
        self.value.learning.insert(position, entry);
    }
    pub fn answer(
        &mut self,
        id: CardId,
        requeue: Option<Requeue>,
        buried: &[CardId],
        now: i64,
    ) -> Result<QueueAnswer, NotAtHead> {
        let removed = self.remove(id)?;
        self.current = None;
        let requeued = requeue.map(|r| {
            let mut entry = LearningEntry {
                id,
                due: r.due,
                reps: r.reps,
                mtime: r.mtime,
            };
            let edge = self.edge();
            if entry.due <= edge
                && self.value.main.is_empty()
                && let Some(first) = self.value.learning.first()
                && first.due >= entry.due
                && first.due.saturating_add(1) < edge
            {
                entry.due = first.due.saturating_add(1);
            }
            if entry.due <= edge {
                self.increment(Kind::Learning);
            }
            self.insert_learning(entry.clone());
            entry
        });
        let mut record = QueueAnswer {
            build: self.build,
            removed,
            requeued,
            learning_after_requeue: self.value.counts.learning,
            cutoff_before_advance: self.value.learning_cutoff,
            siblings: Vec::new(),
        };
        self.advance(now);
        let mut i = 0;
        let mut original = 0;
        while i < self.value.main.len() {
            if buried.contains(&self.value.main[i].id) {
                let entry = self.value.main.remove(i);
                self.decrement(entry.kind);
                record.siblings.push((original, entry));
            } else {
                i += 1;
            }
            original += 1;
        }
        Ok(record)
    }
    pub fn predict(
        &self,
        id: CardId,
        requeue: Option<Requeue>,
        buried: &[CardId],
        now: i64,
    ) -> Result<Option<CardId>, NotAtHead> {
        let mut next = self.clone();
        next.answer(id, requeue, buried, now)?;
        Ok(next.read(now, 1).first().map(|e| e.id))
    }
    pub fn undo(&mut self, record: &QueueAnswer) -> Result<(), RestoreError> {
        if record.build != self.build {
            return Err(RestoreError::BuildChanged);
        }
        if let Some(entry) = &record.requeued {
            let at = self
                .value
                .learning
                .iter()
                .position(|e| e.id == entry.id)
                .ok_or(RestoreError::MissingEntry)?;
            self.value.learning.remove(at);
        }
        self.value.counts.learning = record.learning_after_requeue;
        self.value.learning_cutoff = record.cutoff_before_advance;
        if record.requeued.as_ref().is_some_and(|e| e.due <= self.edge()) {
            self.decrement(Kind::Learning);
        }
        for (at, entry) in &record.siblings {
            self.value.main.insert((*at).min(self.value.main.len()), entry.clone());
            if matches!(entry.kind, Kind::New | Kind::Review) {
                self.increment(entry.kind);
            }
        }
        self.increment(record.removed.kind());
        match &record.removed {
            Removed::Main(e) => self.value.main.insert(0, e.clone()),
            Removed::Learning(e) => self.value.learning.insert(0, e.clone()),
        }
        // Compatibility owners never mark a card shown. Production owners explicitly do.
        self.current = None;
        Ok(())
    }
    pub fn redo(&mut self, record: &QueueAnswer) -> Result<(), RestoreError> {
        if record.build != self.build {
            return Err(RestoreError::BuildChanged);
        }
        self.remove(record.removed.id())
            .map_err(|_| RestoreError::MissingEntry)?;
        if let Some(entry) = &record.requeued {
            self.insert_learning(entry.clone());
        }
        self.value.counts.learning = record.learning_after_requeue;
        self.value.learning_cutoff = record.cutoff_before_advance;
        for (_, entry) in &record.siblings {
            if let Some(at) = self.value.main.iter().position(|e| e.id == entry.id) {
                let entry = self.value.main.remove(at);
                self.decrement(entry.kind);
            }
        }
        self.current = None;
        Ok(())
    }
}
