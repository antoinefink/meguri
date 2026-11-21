//! G2–G3: strict front analysis with saved fields and special names.
use meguri_model::{note::Note, notetype::Notetype};
use meguri_template::{Form, ParsedTemplate};
use std::collections::BTreeSet;

pub fn required(nt: &Notetype, note: &Note) -> BTreeSet<u16> {
    Fronts::of(nt).required(nt, note)
}

/// The note type's front templates parsed once, for G2 over many notes of
/// that note type (a bulk rewrite).
#[derive(Debug)]
pub struct Fronts {
    parsed: Vec<ParsedTemplate>,
}

impl Fronts {
    pub fn of(nt: &Notetype) -> Fronts {
        Fronts {
            parsed: nt
                .templates
                .iter()
                .map(|template| ParsedTemplate::parse(&template.config.q_format))
                .collect(),
        }
    }

    /// G2 for `note`, of the note type these fronts were parsed from.
    pub fn required(&self, nt: &Notetype, note: &Note) -> BTreeSet<u16> {
        required_with(nt, note, &self.parsed)
    }
}

fn required_with(nt: &Notetype, note: &Note, fronts: &[ParsedTemplate]) -> BTreeSet<u16> {
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
    fronts
        .iter()
        .enumerate()
        .filter_map(|(ord, parsed)| {
            parsed
                .renders_something(nonempty, Form::Strict)
                .then(|| u16::try_from(ord).ok())
                .flatten()
        })
        .collect()
}
