//! Save normalization (`specs/notetypes/records.md` N4) and the structural
//! rules every save applies (`notetypes/validation` V2), and the check of a
//! field name the user types (V5, R921). Names compare under `unicase`
//! (`text/unicode-primitives` U2) and are stored in NFC (U3).

use std::collections::HashSet;

use meguri_model::ids::NotetypeId;
use meguri_model::notetype::{Notetype, NotetypeField};
use meguri_text::fold::folded;
use meguri_text::normalize::nfc;

/// Why the structural rules or the normalization refuse a save
/// (`invalid_notetype`). Positions are the field's or template's ordinal.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum InvalidNotetype {
    /// V2: the note type has no field.
    NoField,
    /// V2: the note type has no template.
    NoTemplate,
    /// N4 step 1: the note type name is empty once `"` is removed.
    EmptyName,
    /// N4 step 3: a field name is empty, or empty after its fixes.
    EmptyFieldName { ord: usize },
    /// N4 step 4: a template name is empty, or made only of `"`.
    EmptyTemplateName { ord: usize },
}

/// N4 steps 1 to 7 and V2's counts, in that order: the note type name, the
/// field and template names, their uniqueness within the note type, the
/// sort field and the note type name's uniqueness among `others` (the other
/// note types of the collection, by id). Fields and templates get their
/// positions as `ord`. A refusal leaves the note type half normalized: the
/// save writes nothing.
pub fn normalize(notetype: &mut Notetype, others: &[(NotetypeId, String)]) -> Result<(), InvalidNotetype> {
    if notetype.fields.is_empty() {
        return Err(InvalidNotetype::NoField);
    }
    if notetype.templates.is_empty() {
        return Err(InvalidNotetype::NoTemplate);
    }
    // Steps 1 and 2 for the note type name.
    notetype.name = notetype_name(&notetype.name)?;
    // Steps 2 and 3: fields.
    for (ord, field) in notetype.fields.iter_mut().enumerate() {
        field.name = field_name(&field.name).ok_or(InvalidNotetype::EmptyFieldName { ord })?;
    }
    // Steps 2 and 4: templates.
    for (ord, template) in notetype.templates.iter_mut().enumerate() {
        template.name = template_name(&template.name).ok_or(InvalidNotetype::EmptyTemplateName { ord })?;
    }
    // Step 5: templates first, then fields.
    unique_in_list(notetype.templates.iter_mut().map(|t| &mut t.name));
    unique_in_list(notetype.fields.iter_mut().map(|f| &mut f.name));
    // Step 6.
    notetype.config.sort_field_idx = sort_field(notetype);
    // Step 7.
    notetype.name = unique_among(&notetype.name, notetype.id, others);
    number(notetype);
    Ok(())
}

/// N4 steps 1, 2 and 7 on the note type name only, with V2's counts (a
/// clone, N7: its fields and templates are the source's as they are).
pub fn normalize_name(notetype: &mut Notetype, others: &[(NotetypeId, String)]) -> Result<(), InvalidNotetype> {
    if notetype.fields.is_empty() {
        return Err(InvalidNotetype::NoField);
    }
    if notetype.templates.is_empty() {
        return Err(InvalidNotetype::NoTemplate);
    }
    let name = notetype_name(&notetype.name)?;
    notetype.name = unique_among(&name, notetype.id, others);
    number(notetype);
    Ok(())
}

/// N1: positions as `ord`, from 0 without gaps.
fn number(notetype: &mut Notetype) {
    for (ord, field) in notetype.fields.iter_mut().enumerate() {
        field.ord = position(ord);
    }
    for (ord, template) in notetype.templates.iter_mut().enumerate() {
        template.ord = position(ord);
    }
}

fn position(ord: usize) -> u32 {
    u32::try_from(ord).unwrap_or(u32::MAX)
}

/// Steps 1 and 2: every `"` removed, no trimming, then NFC.
fn notetype_name(name: &str) -> Result<String, InvalidNotetype> {
    let name: String = name.chars().filter(|&c| c != '"').collect();
    if name.is_empty() {
        return Err(InvalidNotetype::EmptyName);
    }
    Ok(nfc(&name).into_owned())
}

/// Steps 2 and 3: NFC; refused when empty; every `:`, `{`, `}` and `"`
/// removed; White_Space trimmed at both ends; then `#`, `/`, `^` and
/// White_Space removed from the start; refused when empty.
fn field_name(name: &str) -> Option<String> {
    let name = nfc(name);
    if name.is_empty() {
        return None;
    }
    let kept: String = name.chars().filter(|c| !matches!(c, ':' | '{' | '}' | '"')).collect();
    let fixed = kept
        .trim()
        .trim_start_matches(|c: char| matches!(c, '#' | '/' | '^') || c.is_whitespace());
    (!fixed.is_empty()).then(|| fixed.to_owned())
}

/// Steps 2 and 4: NFC; refused when empty; every `"` removed; refused when
/// empty. No trimming.
fn template_name(name: &str) -> Option<String> {
    let name = nfc(name);
    if name.is_empty() {
        return None;
    }
    let kept: String = name.chars().filter(|&c| c != '"').collect();
    (!kept.is_empty()).then_some(kept)
}

/// Step 5: in list order, a name equal under unicase to an earlier name (as
/// that one ended up) gets `+` until it differs from every earlier name.
fn unique_in_list<'n>(names: impl Iterator<Item = &'n mut String>) {
    let mut earlier: HashSet<String> = HashSet::new();
    for name in names {
        while earlier.contains(&folded(name)) {
            name.push('+');
        }
        earlier.insert(folded(name));
    }
}

/// N4's field-name steps over the complete final list. A rename keeps all
/// other stored data, including legacy ordinals, rather than normalizing
/// unrelated members of the aggregate (`field-rename` F2, F4).
pub(crate) fn normalize_field_names(fields: &mut [NotetypeField]) -> Result<(), InvalidNotetype> {
    if fields.is_empty() {
        return Err(InvalidNotetype::NoField);
    }
    for (ord, field) in fields.iter_mut().enumerate() {
        field.name = field_name(&field.name).ok_or(InvalidNotetype::EmptyFieldName { ord })?;
    }
    unique_in_list(fields.iter_mut().map(|field| &mut field.name));
    Ok(())
}

/// Step 6: the sort field is a field, not a position. A field that carries
/// its stored position (`prior_ord`, a U34 draft) keeps the sort field when
/// it moves; otherwise the index keeps its value, lowered to the last
/// position when it is past the end.
fn sort_field(notetype: &Notetype) -> u32 {
    let stored = notetype.config.sort_field_idx;
    if let Some(at) = notetype.fields.iter().position(|f| f.prior_ord == Some(stored)) {
        return position(at);
    }
    stored.min(position(notetype.fields.len().saturating_sub(1)))
}

/// Step 7: `+` appended until no other note type has a name equal under
/// unicase. A note type never collides with itself.
fn unique_among(name: &str, id: NotetypeId, others: &[(NotetypeId, String)]) -> String {
    let taken: HashSet<String> = others
        .iter()
        .filter(|(other, _)| *other != id)
        .map(|(_, name)| folded(name))
        .collect();
    let mut name = name.to_owned();
    while taken.contains(&folded(&name)) {
        name.push('+');
    }
    name
}

/// V5: what the field editor does with a name the user typed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FieldNameCheck {
    /// Empty once `"` is removed and the ends trimmed: the editor ignores
    /// it.
    Ignored,
    /// The name to use: the typed text without `"`, trimmed.
    Accepted(String),
    Refused(FieldNameRefusal),
}

/// Why V5 refuses a typed field name; the editor explains it in its own
/// words (R921).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FieldNameRefusal {
    /// It starts with `#`, `^` or `/`.
    BadFirstCharacter,
    /// It holds `:`, `{` or `}`.
    BadCharacter,
    /// Another field of the note type has a name equal under unicase.
    Duplicate,
}

/// V5: checks `typed` against the note type's field names; `renaming` is the
/// position of the field being renamed (its own name does not count, so a
/// case-only rename is accepted), `None` for a new field.
pub fn check_typed_field_name(typed: &str, field_names: &[&str], renaming: Option<usize>) -> FieldNameCheck {
    let without_quotes: String = typed.chars().filter(|&c| c != '"').collect();
    let text = without_quotes.trim();
    if text.is_empty() {
        return FieldNameCheck::Ignored;
    }
    if text.starts_with(['#', '^', '/']) {
        return FieldNameCheck::Refused(FieldNameRefusal::BadFirstCharacter);
    }
    if text.contains([':', '{', '}']) {
        return FieldNameCheck::Refused(FieldNameRefusal::BadCharacter);
    }
    let fold = folded(text);
    let duplicate = field_names
        .iter()
        .enumerate()
        .any(|(at, name)| Some(at) != renaming && folded(name) == fold);
    if duplicate {
        return FieldNameCheck::Refused(FieldNameRefusal::Duplicate);
    }
    FieldNameCheck::Accepted(text.to_owned())
}
