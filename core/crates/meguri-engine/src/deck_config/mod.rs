//! Deck preset primitives (specs/decks/deck-presets.md P2-P11).
pub mod ops;
pub use ops::*;

use crate::EngineError;
use meguri_model::{
    deck_config::DeckConfig,
    ids::{DeckConfigId, DeckId},
    proto::{ProtoBlob, deck::DeckKind, deck_config::DeckConfigMessage},
    time::EpochSecs,
    usn::Usn,
};
use meguri_storage::ReadRows;

/// P2, B5: built-in defaults, without FSRS parameter lists.
pub fn defaults() -> DeckConfig {
    let mut config = DeckConfigMessage::default();
    config.learn_steps = vec![1.0, 10.0];
    config.relearn_steps = vec![10.0];
    config.easy_days_percentages = vec![1.0; 7];
    config.new_per_day = 20;
    config.reviews_per_day = 200;
    config.initial_ease = 2.5;
    config.easy_multiplier = 1.3;
    config.hard_multiplier = 1.2;
    config.interval_multiplier = 1.0;
    config.maximum_review_interval = 36500;
    config.minimum_lapse_interval = 1;
    config.graduating_interval_good = 1;
    config.graduating_interval_easy = 4;
    config.leech_action = 1;
    config.leech_threshold = 8;
    config.cap_answer_time_to_secs = 60;
    config.desired_retention = 0.9;
    config.historical_retention = 0.9;
    config.wait_for_audio = true;
    DeckConfig {
        id: DeckConfigId(1),
        name: "Default".into(),
        mtime: EpochSecs(0),
        usn: Usn(0),
        config: ProtoBlob::new(config),
    }
}

pub(crate) fn require(rows: &impl ReadRows, id: DeckConfigId) -> Result<DeckConfig, EngineError> {
    rows.deck_config(id)?.ok_or_else(|| EngineError::NotFound {
        table: meguri_storage::Table::DeckConfig,
        key: id.0.to_string(),
    })
}

/// P3: fallback is read-only. Filtered decks have no preset.
pub fn for_deck(rows: &impl ReadRows, id: DeckId) -> Result<Option<DeckConfig>, EngineError> {
    let deck = crate::decks::require(rows, id)?;
    let DeckKind::Normal(normal) = &deck.kind.kind else {
        return Ok(None);
    };
    Ok(Some(match rows.deck_config(DeckConfigId(normal.config_id))? {
        Some(config) => config,
        None => rows.deck_config(DeckConfigId(1))?.unwrap_or_else(defaults),
    }))
}
pub fn use_count(rows: &impl ReadRows, id: DeckConfigId) -> Result<usize, EngineError> {
    Ok(rows
        .deck_rows()?
        .iter()
        .filter(|d| matches!(&d.kind.kind, DeckKind::Normal(n) if n.config_id == id.0))
        .count())
}

/// P7. The menu passes the other presets' names (the current one excluded).
pub fn menu_name(
    candidate: &str,
    existing: &[String],
    clock: &dyn meguri_model::clock::Clock,
    trim: bool,
) -> Option<String> {
    let candidate = if trim { candidate.trim() } else { candidate };
    if trim && candidate.is_empty() {
        return None;
    }
    Some(if existing.iter().any(|name| name == candidate) {
        format!("{candidate}{}", clock.now_ms().div_euclid(1000))
    } else {
        candidate.to_owned()
    })
}
