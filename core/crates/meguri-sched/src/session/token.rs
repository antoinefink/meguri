//! C5 and C6 have separate domains; neither is a storage RowDigest.
use meguri_model::{
    card::Card,
    card_data::MemoryState,
    ids::{DeckConfigId, NotetypeId},
    time::EpochSecs,
};
use sha2::{Digest, Sha256};

pub type StateToken = [u8; 32];
pub type RenderKey = [u8; 32];

pub struct StateInputs<'a> {
    pub card: &'a Card,
    /// The exact stored `data` cell, never a re-encoding of CardData.
    pub raw_data: &'a [u8],
    pub preset_id: DeckConfigId,
    pub preset_mtime: Option<EpochSecs>,
    pub home_retention: Option<f32>,
    pub short_term_with_steps: bool,
    pub learn_ahead: u32,
    pub today: u32,
    pub next_day_at: i64,
    /// Some only when this input was resolved from history rather than data.
    pub history_memory: Option<MemoryState>,
    pub history_last_review: Option<i64>,
}
fn optional<T>(hash: &mut Sha256, value: Option<T>, put: impl FnOnce(&mut Sha256, T)) {
    hash.update([u8::from(value.is_some())]);
    if let Some(value) = value {
        put(hash, value);
    }
}
pub fn state_token(input: &StateInputs<'_>) -> StateToken {
    let c = input.card;
    let mut hash = Sha256::new();
    hash.update(b"meguri.session.state.v1\0");
    for value in [
        c.card_type.stored(),
        c.queue.stored(),
        i64::from(c.due),
        i64::from(c.interval),
        i64::from(c.ease_factor),
        i64::from(c.reps),
        i64::from(c.lapses),
        i64::from(c.remaining_steps),
        i64::from(c.original_due),
        c.original_deck_id.0,
        c.deck_id.0,
        c.mtime.0,
    ] {
        hash.update(value.to_le_bytes());
    }
    hash.update((input.raw_data.len() as u64).to_le_bytes());
    hash.update(input.raw_data);
    hash.update(input.preset_id.0.to_le_bytes());
    optional(&mut hash, input.preset_mtime, |h, v| h.update(v.0.to_le_bytes()));
    optional(&mut hash, input.home_retention, |h, v| {
        h.update(v.to_bits().to_le_bytes())
    });
    hash.update([u8::from(input.short_term_with_steps)]);
    hash.update(input.learn_ahead.to_le_bytes());
    hash.update(input.today.to_le_bytes());
    hash.update(input.next_day_at.to_le_bytes());
    optional(&mut hash, input.history_memory, |h, v| {
        h.update(v.stability.to_bits().to_le_bytes());
        h.update(v.difficulty.to_bits().to_le_bytes());
    });
    optional(&mut hash, input.history_last_review, |h, v| h.update(v.to_le_bytes()));
    hash.finalize().into()
}
pub fn render_key(card: &Card, note_mtime: EpochSecs, notetype: NotetypeId, notetype_mtime: EpochSecs) -> RenderKey {
    let mut hash = Sha256::new();
    hash.update(b"meguri.session.render.v1\0");
    for value in [
        card.id.0,
        i64::from(card.ord),
        card.deck_id.0,
        card.original_deck_id.0,
        i64::from(card.flags),
        note_mtime.0,
        notetype.0,
        notetype_mtime.0,
    ] {
        hash.update(value.to_le_bytes());
    }
    hash.finalize().into()
}
