//! The template checks of a checked save (`specs/notetypes/validation.md`
//! V3) and the full list of failures the template editor shows (V4), over
//! U60's parse (`render/template-syntax` T6, T11.1).

use meguri_model::notetype::Notetype;
use meguri_template::ParsedTemplate;

use super::NotetypeKind;

/// The names of `render/special-fields` P1 that are not fields. The `c<N>`
/// section keys are not field references at all (T11.1).
const SPECIAL_NAMES: [&str; 8] = [
    "FrontSide",
    "Card",
    "CardFlag",
    "Deck",
    "Subdeck",
    "Tags",
    "Type",
    "CardID",
];

/// The kinds of V3, in V3's order; their names are T7's.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CheckKind {
    /// Step 1: a front repeats an earlier template's.
    DuplicateFront,
    /// Step 2.1: the front or the back has a structural error.
    TemplateParseError,
    /// Step 2.2: the front references no field.
    NoFrontField,
    /// Step 2.3: a side references a name that is not a field.
    UnknownField,
    /// Step 3: a cloze note type's first template lacks a cloze on a side.
    MissingCloze,
}

impl CheckKind {
    /// The kind `specs/conformance/notetype-ops.md` T7 records.
    pub const fn name(self) -> &'static str {
        match self {
            CheckKind::DuplicateFront => "duplicate_front",
            CheckKind::TemplateParseError => "template_parse_error",
            CheckKind::NoFrontField => "no_front_field",
            CheckKind::UnknownField => "unknown_field",
            CheckKind::MissingCloze => "missing_cloze",
        }
    }
}

/// A side of a template.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TemplateSide {
    Front,
    Back,
}

/// One failure (V4): its kind, the template's position, the side when it
/// applies, the offending name of an unknown field (the first in the
/// template's text) and, for a repeated front, the earlier template's
/// position.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct TemplateFailure {
    pub kind: CheckKind,
    pub template: usize,
    pub side: Option<TemplateSide>,
    pub name: Option<String>,
    pub earlier: Option<usize>,
}

impl TemplateFailure {
    fn new(kind: CheckKind, template: usize, side: Option<TemplateSide>) -> TemplateFailure {
        TemplateFailure {
            kind,
            template,
            side,
            name: None,
            earlier: None,
        }
    }
}

/// V4: every failure, in V3's order: each template whose front repeats an
/// earlier one, then per template the first of steps 2.1 to 2.3, then the
/// cloze failure. Browser formats are not checked.
pub fn template_failures(notetype: &Notetype) -> Vec<TemplateFailure> {
    let mut failures = Vec::new();
    let fronts: Vec<&str> = notetype.templates.iter().map(|t| t.config.q_format.as_str()).collect();
    for (at, front) in fronts.iter().enumerate() {
        if mentions_card(front) {
            continue;
        }
        if let Some(earlier) = fronts[..at].iter().position(|f| f == front) {
            let mut failure = TemplateFailure::new(CheckKind::DuplicateFront, at, Some(TemplateSide::Front));
            failure.earlier = Some(earlier);
            failures.push(failure);
        }
    }
    let fields: Vec<&str> = notetype.fields.iter().map(|f| f.name.as_str()).collect();
    let mut parsed = Vec::with_capacity(notetype.templates.len());
    for (at, template) in notetype.templates.iter().enumerate() {
        let front = ParsedTemplate::parse(&template.config.q_format);
        let back = ParsedTemplate::parse(&template.config.a_format);
        if let Some(failure) = template_failure(at, &front, &back, &fields) {
            failures.push(failure);
        }
        parsed.push((front, back));
    }
    if NotetypeKind::of(notetype) == NotetypeKind::Cloze
        && let Some((front, back)) = parsed.first()
    {
        let side = if front.cloze_references().is_empty() {
            Some(TemplateSide::Front)
        } else if back.cloze_references().is_empty() {
            Some(TemplateSide::Back)
        } else {
            None
        };
        if let Some(side) = side {
            failures.push(TemplateFailure::new(CheckKind::MissingCloze, 0, Some(side)));
        }
    }
    failures
}

/// V3: the failure a checked save is refused with, if any.
pub fn first_failure(notetype: &Notetype) -> Option<TemplateFailure> {
    template_failures(notetype).into_iter().next()
}

/// Step 2 for one template: the first of a parse error, no field on the
/// front and an unknown field.
fn template_failure(
    at: usize,
    front: &ParsedTemplate,
    back: &ParsedTemplate,
    fields: &[&str],
) -> Option<TemplateFailure> {
    for (side, parsed) in [(TemplateSide::Front, front), (TemplateSide::Back, back)] {
        if parsed.structure_error().is_some() {
            return Some(TemplateFailure::new(CheckKind::TemplateParseError, at, Some(side)));
        }
    }
    if front.field_references().is_empty() {
        return Some(TemplateFailure::new(
            CheckKind::NoFrontField,
            at,
            Some(TemplateSide::Front),
        ));
    }
    for (side, parsed) in [(TemplateSide::Front, front), (TemplateSide::Back, back)] {
        let unknown = parsed
            .field_references()
            .into_iter()
            .find(|name| !name.is_empty() && !SPECIAL_NAMES.contains(name) && !fields.contains(name));
        if let Some(name) = unknown {
            let mut failure = TemplateFailure::new(CheckKind::UnknownField, at, Some(side));
            failure.name = Some(name.to_owned());
            return Some(failure);
        }
    }
    None
}

/// Step 1's exception: the front holds `{{`, optional White_Space, `Card`,
/// optional White_Space, `}}`.
fn mentions_card(front: &str) -> bool {
    front.match_indices("{{").any(|(at, open)| {
        let rest = front[at + open.len()..].trim_start();
        rest.strip_prefix("Card")
            .is_some_and(|after| after.trim_start().starts_with("}}"))
    })
}
