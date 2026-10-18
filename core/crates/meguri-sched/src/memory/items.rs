//! A shared selection pass for memory replay and optimizer training.

use fsrs::{FSRS, FSRSError, FSRSItem, FSRSReview};
use meguri_model::{
    card_data::MemoryState,
    ids::{CardId, RevlogId},
    revlog::{RevlogEntry, RevlogKind},
};

use crate::states::engine::elapsed_days;

pub fn is_cram(row: &RevlogEntry) -> bool {
    row.kind == RevlogKind::Filtered && row.ease_factor == 0
}
pub fn is_graded(row: &RevlogEntry) -> bool {
    row.button >= 1 && !is_cram(row)
}
pub fn is_reset(row: &RevlogEntry) -> bool {
    row.kind == RevlogKind::Manual && row.ease_factor == 0
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HistoryForm {
    Memory,
    Training,
}

/// The caller supplies one card's decoded rows in ascending id order.
/// The slice keeps all classes so the final-row and learning-run rules retain
/// their meaning even when a row will not become a review.
#[derive(Debug)]
pub struct History<'a> {
    rows: &'a [RevlogEntry],
    pub complete: bool,
}

impl<'a> History<'a> {
    pub fn select(rows: &'a [RevlogEntry], cutoff: i64, form: HistoryForm) -> Option<Self> {
        let tail_start = rows.iter().rposition(is_reset).unwrap_or(0);
        let mut learning_start = None;
        let mut in_run = false;
        for (index, row) in rows.iter().enumerate().skip(tail_start) {
            if is_cram(row) {
                continue;
            }
            let learning = is_graded(row) && row.kind == RevlogKind::Learning;
            if learning && !in_run {
                learning_start = Some(index);
            }
            in_run = learning;
        }
        if let Some(start) = learning_start {
            let complete = rows[start].id.0 >= cutoff || (form == HistoryForm::Memory && start + 1 == rows.len());
            if complete {
                return Some(Self {
                    rows: &rows[start..],
                    complete: true,
                });
            }
        }
        if form == HistoryForm::Training {
            return None;
        }
        let start = rows
            .iter()
            .enumerate()
            .skip(learning_start.unwrap_or(tail_start))
            .find(|(_, row)| is_graded(row) && (row.interval >= 1 || row.interval <= -86_400) && row.id.0 > cutoff)
            .map(|(index, _)| index)?;
        Some(Self {
            rows: &rows[start..],
            complete: false,
        })
    }

    pub fn kept(&self) -> impl Iterator<Item = &'a RevlogEntry> + '_ {
        self.rows.iter().filter(|row| is_graded(row))
    }

    pub fn reviews(&self, next_day_at: i64) -> Vec<FSRSReview> {
        let mut previous_day: Option<u32> = None;
        self.kept()
            .map(|row| {
                let day = elapsed_days(next_day_at, Some(row.id.0 / 1000));
                let delta_t = previous_day.map(|previous| previous.wrapping_sub(day)).unwrap_or(0);
                previous_day = Some(day);
                FSRSReview {
                    rating: u32::from(row.button),
                    delta_t,
                }
            })
            .collect()
    }

    pub fn memory(&self, model: &FSRS, next_day_at: i64, retention: f32) -> Result<MemoryState, FSRSError> {
        let mut item = FSRSItem {
            reviews: self.reviews(next_day_at),
        };
        let initial = if self.complete {
            None
        } else {
            let first = self.kept().next().expect("a selected history has a graded row");
            let factor = if first.ease_factor == 0 {
                2500
            } else {
                first.ease_factor
            };
            let ease = factor as f32 / 1000.0;
            let mut initial = model.memory_state_from_sm2(ease, first.interval.max(1) as f32, retention)?;
            if ease <= 1.1 {
                initial.difficulty = (ease - 0.1) * 9.0 + 1.0;
            }
            item.reviews.remove(0);
            Some(initial)
        };
        let memory = if item.reviews.is_empty() {
            initial.expect("a complete history always has a review")
        } else {
            model.memory_state(item, initial)?
        };
        Ok(MemoryState {
            stability: memory.stability,
            difficulty: memory.difficulty,
        })
    }
}

#[derive(Debug)]
pub struct TrainingItem {
    pub item: FSRSItem,
    pub card_id: CardId,
    pub review_id: RevlogId,
}

/// One preset's owned training buffers. Consume and drop this before building
/// the next preset; `owned_bytes` counts capacities, not serialized size or RSS.
#[derive(Debug, Default)]
pub struct PresetTraining {
    pub items: Vec<TrainingItem>,
    pub review_count: usize,
}

impl PresetTraining {
    pub fn add_card(&mut self, rows: &[RevlogEntry], cutoff: i64, next_day_at: i64) {
        let Some(history) = History::select(rows, cutoff, HistoryForm::Training) else {
            return;
        };
        let reviews = history.reviews(next_day_at);
        let before = self.items.len();
        for (index, row) in history.kept().enumerate().skip(1) {
            if reviews[index].delta_t > 0 {
                self.items.push(TrainingItem {
                    item: FSRSItem {
                        reviews: reviews[..=index].to_vec(),
                    },
                    card_id: row.card_id,
                    review_id: row.id,
                });
            }
        }
        if self.items.len() != before {
            self.review_count += reviews.len();
        }
    }

    pub fn sort(&mut self) {
        self.items.sort_unstable_by_key(|item| item.review_id);
    }

    /// Outer capacity includes the aligned card-id/tag fields and each Vec
    /// header. Inner capacities include every retained FSRS review buffer.
    /// Allocator metadata and separately measured process peak are excluded.
    pub fn owned_bytes(&self) -> usize {
        self.items.capacity() * std::mem::size_of::<TrainingItem>()
            + self
                .items
                .iter()
                .map(|item| item.item.reviews.capacity() * std::mem::size_of::<FSRSReview>())
                .sum::<usize>()
    }
}
