//! R2/R4: closest passed delay; never reschedule or discard learning.
use meguri_model::{card::CardType, proto::deck_config::DeckConfigMessage};
pub fn remaining(left: u32, old: &[f32], new: &[f32]) -> Option<u32> {
    if old.is_empty() {
        return None;
    }
    let count = (left % 1000) as usize;
    let candidate = if count >= old.len() {
        new.len()
    } else {
        let passed = old[old.len() - count - 1];
        new.iter()
            .rposition(|&delay| delay <= passed)
            .map_or(new.len(), |index| new.len() - index - 1)
    };
    let candidate = u32::try_from(candidate).unwrap_or(u32::MAX).max(1);
    (candidate != left % 1000).then_some(candidate)
}
/// R645 hook for Change Deck: callers resolve P3 fallback, skip cards already
/// in the target deck, then apply this with the move and U46 recompute.
/// Equal lists deliberately still translate (R4).
pub fn preset_change(card_type: CardType, left: u32, old: &DeckConfigMessage, new: &DeckConfigMessage) -> Option<u32> {
    match card_type {
        CardType::Learning => remaining(left, &old.learn_steps, &new.learn_steps),
        CardType::Relearning => remaining(left, &old.relearn_steps, &new.relearn_steps),
        _ => None,
    }
}
