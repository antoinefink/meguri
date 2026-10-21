use super::Rating;
use crate::states::engine::AnswerState;
use meguri_model::{
    ids::{CardId, RevlogId},
    revlog::{RevlogEntry, RevlogKind},
    usn::Usn,
};

/// Use U45 T15's preview directly, including its signed interval conversions.
pub fn row(card: CardId, rating: Rating, chosen: &AnswerState, elapsed_ms: u32) -> RevlogEntry {
    RevlogEntry {
        id: RevlogId(0),
        card_id: card,
        usn: Usn::PENDING,
        button: rating as u8,
        interval: chosen.revlog.interval,
        last_interval: chosen.revlog.last_interval,
        ease_factor: chosen.revlog.factor,
        answer_millis: elapsed_ms,
        kind: match chosen.revlog.kind {
            1 => RevlogKind::Review,
            2 => RevlogKind::Relearning,
            _ => RevlogKind::Learning,
        },
    }
}
