//! D1–D5: walk usable candidates; a broken Default fails the operation.
use crate::EngineError;
use meguri_model::proto::deck::DeckKind;
use meguri_model::{card::Card, ids::DeckId, notetype::Notetype};
use meguri_storage::ReadRows;

pub fn usable(reader: &impl ReadRows, id: DeckId) -> Result<bool, EngineError> {
    Ok(reader
        .deck(id)?
        .is_some_and(|deck| matches!(deck.kind.kind, DeckKind::Normal(_))))
}

pub fn deck(
    reader: &impl ReadRows,
    nt: &Notetype,
    ord: u16,
    fallback: bool,
    siblings: &[Card],
    chosen: Option<DeckId>,
) -> Result<DeckId, EngineError> {
    if !usable(reader, DeckId(1))? {
        return Err(crate::notes::invalid("Default deck is missing or filtered"));
    }
    let mut candidates = Vec::new();
    if nt.config.kind != 1
        && !fallback
        && let Some(template) = nt.templates.get(usize::from(ord))
        && template.config.target_deck_id > 0
    {
        candidates.push(DeckId(template.config.target_deck_id));
    }
    if let Some(chosen) = chosen {
        candidates.push(chosen);
    } else {
        let home = |card: &Card| {
            if card.original_deck_id.0 != 0 {
                card.original_deck_id
            } else {
                card.deck_id
            }
        };
        if let Some(first) = siblings.first()
            && siblings.iter().all(|c| home(c) == home(first))
        {
            candidates.push(home(first));
        }
        if let Some(last) = reader
            .config_value(&format!("_nt_{}_lastDeck", nt.id.0))?
            .and_then(|v| v.as_i64())
        {
            candidates.push(DeckId(last));
        }
    }
    candidates.push(DeckId(1));
    for candidate in candidates {
        if usable(reader, candidate)? {
            return Ok(candidate);
        }
    }
    unreachable!("Default was checked")
}
