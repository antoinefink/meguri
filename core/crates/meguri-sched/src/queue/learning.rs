use super::LearningEntry;
pub fn order(cards: &mut [LearningEntry]) {
    // Stable sorting retains the storage input's ascending did/id ties.
    cards.sort_by_key(|c| (c.reps == 0, c.due));
}
