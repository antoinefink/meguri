//! What a local save computes before its rows are written
//! (`specs/notetypes/records.md` N3, N6): the digest that tells whether a
//! draft's note type is still the stored one (`engine/op-model` O7), the
//! card requirements, when they are computed afresh, and the changes that
//! make a save U34's structure change rather than a plain save. The per-row
//! writes and stamps (N3.1, N3.2) are meguri-storage's
//! `WriteScope::put_notetype`.

use meguri_model::notetype::Notetype;
use meguri_model::proto::notetype::CardRequirement;
use meguri_storage::Cell;
use meguri_storage::journal::digest;
use meguri_template::{ParsedTemplate, RequirementKind, legacy_requirement};

/// The content of a note type aggregate as read (`engine/op-model` O7): its
/// id, name and config, and every field and template row with the ordinal
/// it was read with, the template rows' `mtime_secs` and `usn` included
/// (data, N3.3). The `notetypes` row's own `mtime_secs` and `usn` are left
/// out: every save restamps them, and a sync that only marks the row as sent
/// changes nothing a draft holds. Blobs count by their stored bytes, so two
/// reads of unchanged rows give the same digest.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct AggregateDigest(pub [u8; 32]);

impl AggregateDigest {
    pub fn of(notetype: &Notetype) -> AggregateDigest {
        let count = |n: usize| Cell::Integer(i64::try_from(n).unwrap_or(i64::MAX));
        let mut cells = vec![
            Cell::Integer(notetype.id.0),
            Cell::text(notetype.name.clone()),
            Cell::Blob(notetype.config.to_bytes().into_owned()),
            count(notetype.fields.len()),
        ];
        for field in &notetype.fields {
            cells.push(Cell::Integer(i64::from(field.ord)));
            cells.push(Cell::text(field.name.clone()));
            cells.push(Cell::Blob(field.config.to_bytes().into_owned()));
        }
        cells.push(count(notetype.templates.len()));
        for template in &notetype.templates {
            cells.push(Cell::Integer(i64::from(template.ord)));
            cells.push(Cell::text(template.name.clone()));
            cells.push(Cell::Integer(template.mtime.0));
            cells.push(Cell::Integer(i64::from(template.usn.0)));
            cells.push(Cell::Blob(template.config.to_bytes().into_owned()));
        }
        AggregateDigest(digest(&cells))
    }
}

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
/// added, removed, moved or renamed (fields), template text, the sort field
/// or the kind. A field or template is known by the `ord` it was read with,
/// which moves with it in the draft, and by its id: the ids are optional
/// (legacy note types have none), so a move of two templates with the same
/// texts and no ids would otherwise look like two renames (R949).
pub fn structure_change(stored: &Notetype, draft: &Notetype) -> Option<&'static str> {
    if stored.config.kind != draft.config.kind {
        return Some("a kind change");
    }
    if stored.fields.len() != draft.fields.len()
        || stored
            .fields
            .iter()
            .zip(&draft.fields)
            .any(|(a, b)| a.ord != b.ord || a.config.id != b.config.id)
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
            .any(|(a, b)| a.ord != b.ord || a.config.id != b.config.id)
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
