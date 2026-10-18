//! Editor draft check (field-check K1–K4), with indexed checksum candidates.
use crate::EngineError;
use meguri_model::{ids::NoteId, notetype::Notetype};
use meguri_storage::ReadRows;
use meguri_template::{ParsedTemplate, cloze::has_cloze};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FieldState {
    Normal,
    Empty,
    Duplicate,
    MissingCloze,
    NotetypeNotCloze,
    FieldNotCloze,
}
impl FieldState {
    pub fn name(self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::Empty => "empty",
            Self::Duplicate => "duplicate",
            Self::MissingCloze => "missing_cloze",
            Self::NotetypeNotCloze => "notetype_not_cloze",
            Self::FieldNotCloze => "field_not_cloze",
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FieldCheck {
    pub state: FieldState,
    pub duplicates: Vec<NoteId>,
}

pub fn check(
    reader: &impl ReadRows,
    nt: &Notetype,
    fields: &[String],
    own: Option<NoteId>,
) -> Result<FieldCheck, EngineError> {
    if fields.len() != nt.fields.len() || fields.is_empty() {
        return Err(super::invalid("wrong field count"));
    }
    let clozes: Vec<_> = fields.iter().map(|f| has_cloze(f)).collect();
    let is_cloze = nt.config.kind == 1;
    let parsed = nt.templates.first().map(|t| ParsedTemplate::parse(&t.config.q_format));
    let cloze_fields = parsed
        .as_ref()
        .filter(|p| p.structure_error().is_none())
        .map(|p| p.cloze_references())
        .unwrap_or_default();
    let misplaced = is_cloze
        && clozes
            .iter()
            .enumerate()
            .any(|(i, has)| *has && !cloze_fields.contains(&nt.fields[i].name.as_str()));
    let normalized = if super::fields::normalize_enabled(reader)? {
        meguri_text::normalize::nfc(&fields[0])
    } else {
        std::borrow::Cow::Borrowed(fields[0].as_str())
    };
    let text = meguri_text::html::strip_markup_keep_media(&normalized);
    let state = if misplaced {
        FieldState::FieldNotCloze
    } else if text.trim().is_empty() {
        FieldState::Empty
    } else if !is_cloze && clozes.iter().any(|v| *v) {
        FieldState::NotetypeNotCloze
    } else if is_cloze && !clozes.iter().any(|v| *v) {
        FieldState::MissingCloze
    } else {
        FieldState::Normal
    };
    let mut duplicates = Vec::new();
    if state == FieldState::Normal {
        let checksum = meguri_text::checksum::note_checksum(&text);
        for note in reader.notes_with_checksum(nt.id, checksum)? {
            if Some(note.id) != own
                && meguri_text::html::strip_markup_keep_media(note.field_values().next().unwrap_or_default()) == text
            {
                duplicates.push(note.id);
            }
        }
        duplicates.sort_unstable();
    }
    Ok(FieldCheck {
        state: if duplicates.is_empty() {
            state
        } else {
            FieldState::Duplicate
        },
        duplicates,
    })
}
