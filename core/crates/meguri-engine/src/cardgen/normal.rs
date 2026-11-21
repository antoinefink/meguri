//! G2–G3: strict front analysis with saved fields and special names.
use meguri_model::{note::Note, notetype::Notetype};
use meguri_template::{Form, ParsedTemplate};
use std::collections::BTreeSet;

/// G2's special names that count as non-empty for every note (unless a
/// field has the name); `Tags` counts when the note has a tag.
pub(crate) const ALWAYS_NON_EMPTY: [&str; 6] = ["Type", "Deck", "Subdeck", "Card", "CardFlag", "CardID"];

pub fn required(nt: &Notetype, note: &Note) -> BTreeSet<u16> {
    required_by(nt, &fronts(nt), note)
}

/// The note type's fronts, parsed, in template order (none for a cloze
/// note type, whose templates play no part in G4).
pub fn fronts(nt: &Notetype) -> Vec<ParsedTemplate> {
    if nt.config.kind == 1 {
        return Vec::new();
    }
    nt.templates
        .iter()
        .map(|template| ParsedTemplate::parse(&template.config.q_format))
        .collect()
}

/// [`required`] with the note type's fronts parsed once, in template order,
/// for a pass over many notes.
pub fn required_by(nt: &Notetype, fronts: &[ParsedTemplate], note: &Note) -> BTreeSet<u16> {
    let values: Vec<_> = note.field_values().collect();
    let nonempty = |name: &str| {
        if let Some(index) = nt.fields.iter().position(|field| field.name == name) {
            return values
                .get(index)
                .is_some_and(|value| !meguri_text::field_empty::counts_as_empty(value));
        }
        match name {
            name if ALWAYS_NON_EMPTY.contains(&name) => true,
            "Tags" => crate::tags::split_text(&note.tags).next().is_some(),
            _ => false,
        }
    };
    fronts
        .iter()
        .enumerate()
        .filter_map(|(ord, front)| {
            front
                .renders_something(nonempty, Form::Strict)
                .then(|| u16::try_from(ord).ok())
                .flatten()
        })
        .collect()
}
