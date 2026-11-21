//! Missing-card generation (specs/notes/card-generation G1–G10).
pub mod cloze;
pub mod normal;
pub mod placement;
pub mod position;
pub mod regenerate;

use crate::EngineError;
use meguri_model::{
    card::{Card, CardQueue, CardType},
    card_data::CardData,
    ids::{CardId, DeckId},
    note::Note,
    notetype::Notetype,
    usn::Usn,
};
use meguri_storage::{ReadRows, WriteScope};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MissingCard {
    pub ord: u16,
    pub deck: DeckId,
}

/// G6's fallback is optional because whole-type regeneration excludes it.
pub fn plan_note(
    reader: &impl ReadRows,
    nt: &Notetype,
    note: &Note,
    siblings: &[Card],
    chosen: Option<DeckId>,
    allow_fallback: bool,
) -> Result<Vec<MissingCard>, EngineError> {
    plan_note_with(reader, nt, None, note, siblings, chosen, allow_fallback)
}

/// [`plan_note`] with the note type's fronts parsed once by the caller
/// (`normal::Fronts`), for many notes of one note type.
pub fn plan_note_with(
    reader: &impl ReadRows,
    nt: &Notetype,
    fronts: Option<&normal::Fronts>,
    note: &Note,
    siblings: &[Card],
    chosen: Option<DeckId>,
    allow_fallback: bool,
) -> Result<Vec<MissingCard>, EngineError> {
    let mut required = if nt.config.kind == 1 {
        cloze::required(note)
    } else {
        match fronts {
            Some(fronts) => fronts.required(nt, note),
            None => normal::required(nt, note),
        }
    };
    let fallback = allow_fallback && siblings.is_empty() && required.is_empty();
    if fallback {
        required.insert(0);
    }
    required
        .into_iter()
        .filter(|ord| !siblings.iter().any(|card| card.ord == *ord))
        .map(|ord| {
            Ok(MissingCard {
                ord,
                deck: placement::deck(reader, nt, ord, fallback, siblings, chosen)?,
            })
        })
        .collect()
}

pub fn generate_in(
    scope: &mut WriteScope<'_>,
    nt: &Notetype,
    note: &Note,
    chosen: Option<DeckId>,
) -> Result<Vec<CardId>, EngineError> {
    let siblings = scope.cards_of_note(note.id)?;
    let missing = plan_note(scope, nt, note, &siblings, chosen, true)?;
    apply_in(scope, note, &siblings, &missing)
}

pub(crate) fn apply_in(
    scope: &mut WriteScope<'_>,
    note: &Note,
    siblings: &[Card],
    missing: &[MissingCard],
) -> Result<Vec<CardId>, EngineError> {
    if missing.is_empty() {
        return Ok(Vec::new());
    }
    let sibling = position::sibling_position(siblings);
    let counter = match sibling {
        Some(value) => value,
        None => position::take_counter(scope)?,
    };
    let mut ids = Vec::new();
    for planned in missing {
        let due = if sibling.is_none() && position::random_order(scope, planned.deck)? {
            position::random_position(counter)
        } else {
            counter
        };
        let card = Card {
            id: CardId(0),
            note_id: note.id,
            deck_id: planned.deck,
            ord: planned.ord,
            mtime: scope.now().whole_secs(),
            usn: Usn::PENDING,
            card_type: CardType::New,
            queue: CardQueue::New,
            due: due as i32,
            interval: 0,
            ease_factor: 0,
            reps: 0,
            lapses: 0,
            remaining_steps: 0,
            original_due: 0,
            original_deck_id: DeckId(0),
            flags: 0,
            data: CardData::parse("{}"),
        };
        ids.push(scope.add_card(&card)?);
    }
    Ok(ids)
}
