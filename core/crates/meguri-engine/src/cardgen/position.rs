//! P1–P4: one counter draw per note, or the oldest new sibling's position.
use crate::EngineError;
use meguri_model::proto::deck::DeckKind;
use meguri_model::{
    card::{Card, CardType},
    ids::{DeckConfigId, DeckId},
};
use meguri_storage::{ConfigValue, ReadRows, WriteScope};
use rand::{Rng, SeedableRng, rngs::StdRng};

pub fn random_position(counter: u32) -> u32 {
    StdRng::seed_from_u64(u64::from(counter)).random_range(1..counter.max(1000))
}

pub fn sibling_position(cards: &[Card]) -> Option<u32> {
    cards
        .iter()
        .filter(|c| c.card_type == CardType::New)
        .min_by_key(|c| c.id)
        .map(|card| {
            (if card.original_due != 0 {
                card.original_due
            } else {
                card.due
            })
            .max(0) as u32
        })
}

pub fn take_counter(scope: &mut WriteScope<'_>) -> Result<u32, EngineError> {
    let counter = read_counter(scope)?;
    scope.set_config("nextPos", &ConfigValue::from(u64::from(counter.wrapping_add(1))))?;
    Ok(counter)
}

pub fn read_counter(rows: &impl ReadRows) -> Result<u32, EngineError> {
    Ok(rows
        .config_value("nextPos")?
        .and_then(|v| v.as_u64())
        .and_then(|v| u32::try_from(v).ok())
        .unwrap_or(0))
}
pub(crate) fn write_counter(scope: &mut WriteScope<'_>, value: u32) -> Result<(), EngineError> {
    let value = ConfigValue::from(u64::from(value));
    if scope.config_value("nextPos")?.as_ref() != Some(&value) {
        scope.put_config_entry(&meguri_model::config::ConfigEntry {
            key: "nextPos".into(),
            value: value.compact().into_bytes(),
            mtime: scope.now().whole_secs(),
            usn: meguri_model::usn::Usn::PENDING,
        })?;
    }
    Ok(())
}

pub fn random_order(reader: &impl ReadRows, id: DeckId) -> Result<bool, EngineError> {
    let preset = match reader.deck(id)?.map(|deck| deck.kind.kind.clone()) {
        Some(DeckKind::Normal(normal)) => DeckConfigId(normal.config_id),
        _ => DeckConfigId(1),
    };
    let config = match reader.deck_config(preset)? {
        Some(config) => Some(config),
        None => reader.deck_config(DeckConfigId(1))?,
    };
    Ok(config.is_some_and(|config| config.config.new_card_insert_order == 1))
}
