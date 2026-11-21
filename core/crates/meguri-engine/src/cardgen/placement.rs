//! D1–D5: walk usable candidates; a broken Default fails the operation.
use crate::EngineError;
use meguri_model::proto::deck::DeckKind;
use meguri_model::{card::Card, ids::DeckId, notetype::Notetype};
use meguri_storage::ReadRows;
use std::cell::RefCell;
use std::collections::HashMap;

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
    deck_with(reader, &Memo::default(), nt, ord, fallback, siblings, chosen)
}

/// What placement reads that does not depend on the note: whether a deck
/// is usable and the note type's last deck. A pass over many notes on one
/// snapshot reads each once.
#[derive(Default)]
pub(crate) struct Memo {
    usable: RefCell<HashMap<DeckId, bool>>,
    /// By note type id: one memo serves one note type, but a key keeps a
    /// reuse across types right.
    last_deck: RefCell<HashMap<i64, Option<i64>>>,
}

impl Memo {
    fn usable(&self, reader: &impl ReadRows, id: DeckId) -> Result<bool, EngineError> {
        if let Some(known) = self.usable.borrow().get(&id) {
            return Ok(*known);
        }
        let found = usable(reader, id)?;
        self.usable.borrow_mut().insert(id, found);
        Ok(found)
    }

    fn last_deck(&self, reader: &impl ReadRows, nt: &Notetype) -> Result<Option<i64>, EngineError> {
        if let Some(known) = self.last_deck.borrow().get(&nt.id.0) {
            return Ok(*known);
        }
        let found = reader
            .config_value(&format!("_nt_{}_lastDeck", nt.id.0))?
            .and_then(|v| v.as_i64());
        self.last_deck.borrow_mut().insert(nt.id.0, found);
        Ok(found)
    }
}

/// [`deck`] with `memo`'s reads, which must come from `reader`'s rows.
pub(crate) fn deck_with(
    reader: &impl ReadRows,
    memo: &Memo,
    nt: &Notetype,
    ord: u16,
    fallback: bool,
    siblings: &[Card],
    chosen: Option<DeckId>,
) -> Result<DeckId, EngineError> {
    if !memo.usable(reader, DeckId(1))? {
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
        if let Some(last) = memo.last_deck(reader, nt)? {
            candidates.push(DeckId(last));
        }
    }
    candidates.push(DeckId(1));
    for candidate in candidates {
        if memo.usable(reader, candidate)? {
            return Ok(candidate);
        }
    }
    unreachable!("Default was checked")
}
