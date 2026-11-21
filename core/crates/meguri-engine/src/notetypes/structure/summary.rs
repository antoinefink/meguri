//! The save summary of `notetypes/draft-save` D2, as the structure ops
//! compute it from a plan and the counts read with it: what `LossesChanged`
//! carries (S1.2, read in the refusing transaction), what U267's
//! `draft_summary` read will return from a planning snapshot, and the core
//! of `restore-to-default` D7.1's `restore_summary`.

use meguri_storage::journal::StructureReason;

use super::plan::{Counts, Layout};
use crate::SchemaReason;
use crate::notetypes::AggregateDigest;
use crate::cardgen::regenerate::RegenerationPlan;

/// D2: the reasons, the changes in D2's order and whether content is lost.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DraftSummary {
    pub reasons: Vec<SchemaReason>,
    pub changes: Vec<DraftChange>,
    /// A removed field has notes with content, or a removed card type (or a
    /// restore's kind change) has cards.
    pub loses_content: bool,
}

/// `restore-to-default` D7.1: D2's summary of the by-position draft, the
/// cards a cloze-to-standard restore removes (D4.1's first bullet), and the
/// digest of the note type as read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RestoreSummary {
    pub summary: DraftSummary,
    pub kind_change_cards_removed: u64,
    pub digest: AggregateDigest,
}

impl RestoreSummary {
    /// The confirmed losses a caller passes after showing this summary.
    pub fn confirmed(&self) -> super::ConfirmedLosses {
        let mut confirmed = super::ConfirmedLosses {
            kind_change_cards: self.kind_change_cards_removed,
            ..Default::default()
        };
        for change in &self.summary.changes {
            match change {
                DraftChange::FieldRemoved { ord, notes_with_content, .. } => {
                    confirmed.fields.insert(*ord, *notes_with_content);
                }
                DraftChange::TemplateRemoved { ord, cards_removed, .. } => {
                    confirmed.templates.insert(*ord, *cards_removed);
                }
                _ => {}
            }
        }
        confirmed
    }
}

/// One change of D2's list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DraftChange {
    FieldAdded { name: String },
    /// `ord` is the removed field's base ordinal.
    FieldRemoved { ord: u32, name: String, notes_with_content: u64 },
    FieldRenamed { from: String, to: String },
    FieldsReordered,
    SortFieldChanged { name: String },
    TemplateAdded { name: String, cards_created: u64 },
    /// `notes_without_cards` is S11's over every removed base ordinal
    /// together.
    /// `ord` is the removed template's base ordinal.
    TemplateRemoved { ord: u32, name: String, cards_removed: u64, notes_without_cards: u64 },
    TemplateRenamed { from: String, to: String },
    TemplatesReordered,
    NotetypeRenamed { from: String, to: String },
    TextOrStylingChanged,
}

/// Whether the kept parts' origins are out of their base order.
fn reordered(origins: &[Option<u32>]) -> bool {
    let kept: Vec<u32> = origins.iter().flatten().copied().collect();
    kept.windows(2).any(|pair| pair[0] > pair[1])
}

pub(crate) fn of(layout: &Layout, counts: &Counts, generation: Option<&RegenerationPlan>) -> DraftSummary {
    let base = &layout.base;
    let saved = &layout.saved;
    let mut changes = Vec::new();
    for (field, origin) in saved.fields.iter().zip(&layout.field_origins) {
        if origin.is_none() {
            changes.push(DraftChange::FieldAdded {
                name: field.name.clone(),
            });
        }
    }
    for &removed in &layout.removed_fields {
        changes.push(DraftChange::FieldRemoved {
            ord: removed,
            name: base.fields[removed as usize].name.clone(),
            notes_with_content: counts.fields.get(&removed).copied().unwrap_or(0),
        });
    }
    for (field, origin) in saved.fields.iter().zip(&layout.field_origins) {
        if let Some(o) = origin {
            let old = &base.fields[*o as usize].name;
            if *old != field.name {
                changes.push(DraftChange::FieldRenamed {
                    from: old.clone(),
                    to: field.name.clone(),
                });
            }
        }
    }
    if reordered(&layout.field_origins) {
        changes.push(DraftChange::FieldsReordered);
    }
    if layout.structure_reasons.contains(&StructureReason::SortFieldChanged) {
        let name = saved
            .fields
            .get(saved.config.sort_field_idx as usize)
            .map(|f| f.name.clone())
            .unwrap_or_default();
        changes.push(DraftChange::SortFieldChanged { name });
    }
    for (at, (template, origin)) in saved.templates.iter().zip(&layout.template_origins).enumerate() {
        if origin.is_none() {
            let cards_created = generation.map_or(0, |plan| {
                plan.notes
                    .iter()
                    .flat_map(|note| &note.cards)
                    .filter(|card| usize::from(card.ord) == at)
                    .count() as u64
            });
            changes.push(DraftChange::TemplateAdded {
                name: template.name.clone(),
                cards_created,
            });
        }
    }
    for (&ord, &cards_removed) in &counts.templates {
        changes.push(DraftChange::TemplateRemoved {
            ord,
            name: base.templates[ord as usize].name.clone(),
            cards_removed,
            notes_without_cards: counts.notes_without_cards,
        });
    }
    for (template, origin) in saved.templates.iter().zip(&layout.template_origins) {
        if let Some(o) = origin {
            let old = &base.templates[*o as usize].name;
            if *old != template.name {
                changes.push(DraftChange::TemplateRenamed {
                    from: old.clone(),
                    to: template.name.clone(),
                });
            }
        }
    }
    if reordered(&layout.template_origins) {
        changes.push(DraftChange::TemplatesReordered);
    }
    if base.name != saved.name {
        changes.push(DraftChange::NotetypeRenamed {
            from: base.name.clone(),
            to: saved.name.clone(),
        });
    }
    if layout.text_or_styling_changed {
        changes.push(DraftChange::TextOrStylingChanged);
    }
    let loses_content = counts.fields.values().any(|n| *n > 0)
        || counts.templates.values().any(|n| *n > 0)
        || counts.kind_change_cards > 0;
    DraftSummary {
        reasons: layout.reasons.clone(),
        changes,
        loses_content,
    }
}
