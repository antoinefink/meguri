//! What a local save computes before its rows are written
//! (`specs/notetypes/records.md` N3, N6): the card requirements, when they
//! are computed afresh, and the changes that make a save U34's structure
//! change rather than a plain save. The per-row writes and stamps (N3.1,
//! N3.2) are meguri-storage's `WriteScope::put_notetype`.

use meguri_model::notetype::Notetype;
use meguri_model::proto::notetype::CardRequirement;
use meguri_template::{ParsedTemplate, RequirementKind, legacy_requirement};

/// N6: one requirement per template, in template order, from its front by
/// `render/template-syntax` T11.3 with the note type's own field names.
pub fn card_requirements(notetype: &Notetype) -> Vec<CardRequirement> {
    let names: Vec<&str> = notetype.fields.iter().map(|f| f.name.as_str()).collect();
    notetype
        .templates
        .iter()
        .enumerate()
        .map(|(ord, template)| {
            let found = legacy_requirement(&ParsedTemplate::parse(&template.config.q_format), &names);
            let mut requirement = CardRequirement::default();
            requirement.card_ord = u32::try_from(ord).unwrap_or(u32::MAX);
            requirement.kind = match found.kind {
                RequirementKind::None => 0,
                RequirementKind::Any => 1,
                RequirementKind::All => 2,
            };
            requirement.field_ords = found
                .ords
                .into_iter()
                .map(|ord| u32::try_from(ord).unwrap_or(u32::MAX))
                .collect();
            requirement
        })
        .collect()
}

/// N6: a save computes the requirements afresh when it changes a front, the
/// field list (count, order or a name) or the kind; any other save keeps
/// the stored ones as read.
pub fn requirements_stale(stored: &Notetype, saved: &Notetype) -> bool {
    stored.config.kind != saved.config.kind
        || stored.fields.len() != saved.fields.len()
        || stored.fields.iter().zip(&saved.fields).any(|(a, b)| a.name != b.name)
        || stored.templates.len() != saved.templates.len()
        || stored
            .templates
            .iter()
            .zip(&saved.templates)
            .any(|(a, b)| a.config.q_format != b.config.q_format)
}

/// The first change of `draft` against `stored` that only U34's draft apply
/// may save (`specs/conformance/notetype-ops.md` T5): fields or templates
/// added, removed, moved (ids by position) or renamed (fields), template
/// text, the sort field or the kind.
pub fn structure_change(stored: &Notetype, draft: &Notetype) -> Option<&'static str> {
    if stored.config.kind != draft.config.kind {
        return Some("a kind change");
    }
    if stored.fields.len() != draft.fields.len()
        || stored
            .fields
            .iter()
            .zip(&draft.fields)
            .any(|(a, b)| a.config.id != b.config.id)
    {
        return Some("a change of the field list");
    }
    if stored.fields.iter().zip(&draft.fields).any(|(a, b)| a.name != b.name) {
        return Some("a field rename");
    }
    if stored.config.sort_field_idx != draft.config.sort_field_idx {
        return Some("a sort field change");
    }
    if stored.templates.len() != draft.templates.len()
        || stored
            .templates
            .iter()
            .zip(&draft.templates)
            .any(|(a, b)| a.config.id != b.config.id)
    {
        return Some("a change of the template list");
    }
    if stored
        .templates
        .iter()
        .zip(&draft.templates)
        .any(|(a, b)| a.config.q_format != b.config.q_format || a.config.a_format != b.config.a_format)
    {
        return Some("a template text change");
    }
    None
}
