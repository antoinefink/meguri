//! G2–G3: strict front analysis with saved fields and special names.
use meguri_model::{note::Note, notetype::Notetype};
use meguri_template::{Form, ParsedTemplate};
use std::collections::BTreeSet;

pub fn required(nt: &Notetype, note: &Note) -> BTreeSet<u16> {
    let values: Vec<_> = note.field_values().collect();
    let nonempty = |name: &str| {
        if let Some(index) = nt.fields.iter().position(|field| field.name == name) {
            return values
                .get(index)
                .is_some_and(|value| !meguri_text::field_empty::counts_as_empty(value));
        }
        match name {
            "Type" | "Deck" | "Subdeck" | "Card" | "CardFlag" | "CardID" => true,
            "Tags" => crate::tags::split_text(&note.tags).next().is_some(),
            _ => false,
        }
    };
    nt.templates
        .iter()
        .enumerate()
        .filter_map(|(ord, template)| {
            ParsedTemplate::parse(&template.config.q_format)
                .renders_something(nonempty, Form::Strict)
                .then(|| u16::try_from(ord).ok())
                .flatten()
        })
        .collect()
}
