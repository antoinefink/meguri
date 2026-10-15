//! The field map and rendering one side (`specs/render/template-syntax.md`
//! T1, T7, T8; `special-fields.md` P2-P5).

use std::collections::HashMap;

use meguri_text::field_empty;

use crate::analysis::Form;
use crate::error::{ErrorKind, TemplateError};
use crate::filters;
use crate::parser::{NodeKind, ParsedTemplate, error};

/// The special whose replacement is resolved by the engine itself (P4).
pub const FRONT_SIDE: &str = "FrontSide";

/// The `Deck` value of a card whose deck does not exist, which includes a
/// card not yet added (P3).
pub const MISSING_DECK: &str = "(Deck)";

/// What the specials of one card are made from (P3). The caller resolves
/// the deck: the deck `odid` names when it is not 0, else the one `did`
/// names.
#[derive(Debug, Clone, Copy)]
pub struct CardSpecials<'a> {
    /// The card's ordinal, 0-based (for a cloze note type, the cloze number
    /// minus one).
    pub ord: u32,
    pub notetype_name: &'a str,
    /// The template rendered: the one at `ord` for a standard note type,
    /// the first for a cloze note type.
    pub template_name: &'a str,
    /// The deck's full name; `None` when it does not exist (or the card is
    /// not added yet).
    pub deck_name: Option<&'a str>,
    /// The card's whole `flags` column.
    pub flags: i64,
    /// 0 for a card not added yet.
    pub card_id: i64,
    /// The note's tags as stored, in stored order.
    pub tags: &'a [String],
}

/// The names a card's templates can use and their values (P2), plus the
/// card's ordinal.
#[derive(Debug, Clone, Default)]
pub struct FieldMap {
    values: HashMap<String, String>,
    ord: u32,
}

impl FieldMap {
    /// P2: every note field by name; each special unless a note field has
    /// exactly its name; `c<ord+1>` = `1` likewise. `FrontSide` is never
    /// added: the engine resolves it.
    pub fn new<'f, I>(note_fields: I, specials: &CardSpecials<'_>) -> FieldMap
    where
        I: IntoIterator<Item = (&'f str, &'f str)>,
    {
        let mut values: HashMap<String, String> = note_fields
            .into_iter()
            .map(|(name, value)| (name.to_owned(), value.to_owned()))
            .collect();
        let deck = specials.deck_name.unwrap_or(MISSING_DECK);
        let subdeck = deck.rsplit_once("::").map_or(deck, |(_, last)| last);
        let derived = [
            ("Tags", specials.tags.join(" ")),
            ("Type", specials.notetype_name.to_owned()),
            ("Deck", deck.to_owned()),
            ("Subdeck", subdeck.to_owned()),
            ("Card", specials.template_name.to_owned()),
            ("CardFlag", format!("flag{}", specials.flags)),
            ("CardID", specials.card_id.to_string()),
        ];
        for (name, value) in derived {
            values.entry(name.to_owned()).or_insert(value);
        }
        values
            .entry(format!("c{}", u64::from(specials.ord) + 1))
            .or_insert_with(|| "1".to_owned());
        FieldMap {
            values,
            ord: specials.ord,
        }
    }

    pub fn get(&self, name: &str) -> Option<&str> {
        self.values.get(name).map(String::as_str)
    }

    /// Whether `name` is in the map with a value that is not empty
    /// (`text/field-emptiness#E1` on the value as given).
    pub fn is_nonempty(&self, name: &str) -> bool {
        self.get(name).is_some_and(|value| !field_empty::counts_as_empty(value))
    }

    pub fn ord(&self) -> u32 {
        self.ord
    }
}

/// The side being rendered (T1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side<'a> {
    Question,
    /// `front_side` is what `{{FrontSide}}` outputs (P4).
    Answer {
        front_side: &'a str,
    },
}

/// T7: a section key that is not in the map but reads as `c` and an
/// unsigned 32-bit decimal (optionally led by one `+`) is empty, not an
/// error.
pub(crate) fn is_card_number(key: &str) -> bool {
    key.strip_prefix('c').is_some_and(|n| n.parse::<u32>().is_ok())
}

impl ParsedTemplate {
    /// Renders one side (T7): the first structural error if any (T6),
    /// else the first render error in source order, hidden sections
    /// included (T8), else the output.
    pub fn render(&self, fields: &FieldMap, side: Side<'_>) -> Result<String, TemplateError> {
        if let Some(error) = self.structure_error() {
            return Err(error.clone());
        }
        let mut out = String::new();
        // One entry per enclosing section: whether it shows. Output is
        // written only while none is hidden.
        let mut shows: Vec<bool> = Vec::new();
        let mut hidden = 0usize;
        for node in self.nodes() {
            let visible = hidden == 0;
            match &node.kind {
                NodeKind::Prefix => {}
                NodeKind::Text | NodeKind::Comment => {
                    if visible {
                        out.push_str(self.text_of(node));
                    }
                }
                NodeKind::Replacement { key, filters } => {
                    if key == FRONT_SIDE {
                        if let (true, Side::Answer { front_side }) = (visible, side) {
                            out.push_str(front_side);
                        }
                    } else if key.is_empty() && !filters.is_empty() {
                        // T7 rule 2: nothing, and no error.
                    } else if let Some(value) = fields.get(key) {
                        if visible {
                            out.push_str(&filters::apply(value, key, filters));
                        }
                    } else {
                        return Err(error(ErrorKind::UnknownField, node, None));
                    }
                }
                NodeKind::Open { key, negated } => {
                    let empty = match fields.get(key) {
                        Some(value) => field_empty::counts_as_empty(value),
                        None if is_card_number(key) => true,
                        None => return Err(error(ErrorKind::UnknownConditional, node, None)),
                    };
                    let show = empty == *negated;
                    shows.push(show);
                    hidden += usize::from(!show);
                }
                NodeKind::Close { .. } => hidden -= usize::from(shows.pop() == Some(false)),
            }
        }
        Ok(out)
    }

    /// T10 for a standard note type: after the question rendered without
    /// error, whether it is blank, that is whether its strict analysis
    /// (T11.2) finds nothing to render for the names whose values are
    /// non-empty. Cloze note types decide by their cloze numbers instead
    /// (U61).
    pub fn is_blank_question(&self, fields: &FieldMap) -> bool {
        !self.renders_something(|name| fields.is_nonempty(name), Form::Strict)
    }
}
