//! Stored records to the render field map (K3), within the caller's snapshot.
use crate::EngineError;
use meguri_model::{
    card::Card,
    note::Note,
    notetype::{Notetype, NotetypeTemplate},
};
use meguri_storage::ReadRows;
use meguri_template::{CardSpecials, FieldMap};

pub(crate) fn build<R: ReadRows + ?Sized>(
    reader: &R,
    card: &Card,
    note: &Note,
    kind: &Notetype,
    template: &NotetypeTemplate,
) -> Result<FieldMap, EngineError> {
    let home = if card.original_deck_id.0 != 0 {
        card.original_deck_id
    } else {
        card.deck_id
    };
    let deck = reader.deck(home)?.map(|d| d.human_name());
    Ok(from_captured(card, note, kind, template, deck.as_deref()))
}

pub(super) fn from_captured(
    card: &Card,
    note: &Note,
    kind: &Notetype,
    template: &NotetypeTemplate,
    deck: Option<&str>,
) -> FieldMap {
    let tags: Vec<String> = crate::tags::canonical::split_text(&note.tags)
        .map(str::to_owned)
        .collect();
    let values: Vec<&str> = note.field_values().collect();
    FieldMap::new(
        kind.fields
            .iter()
            .map(|f| (f.name.as_str(), values.get(f.ord as usize).copied().unwrap_or(""))),
        &CardSpecials {
            ord: u32::from(card.ord),
            notetype_name: &kind.name,
            template_name: &template.name,
            deck_name: deck,
            flags: i64::from(card.flags),
            card_id: card.id.0,
            tags: &tags,
        },
    )
}
