//! Planning a structure change on a snapshot (S2, S3 steps 1 to 5, S10):
//! the saved note type, what changed, the rewritten templates (S4), the
//! note remap (S5), the card map (S6), the generation plan (S7) and the
//! refusals of S1.2 in their order. The same scans run again on the live
//! rows in the write phase ([`super::commit`]).

use std::collections::{BTreeMap, BTreeSet, HashSet};

use meguri_model::card::Card;
use meguri_model::ids::NotetypeId;
use meguri_model::note::Note;
use meguri_model::notetype::{Notetype, NotetypeField, NotetypeTemplate};
use meguri_model::proto::notetype::CardRequirement;
use meguri_storage::ReadRows;
use meguri_storage::journal::{LayoutReport, LayoutShape, StructureReason, StructureReport};

use super::summary::{self, RestoreSummary};
use super::{
    ConfirmedLosses, DraftTemplate, Losses, StructureDraft, StructureRefusal, invalid, position, stale, write_spec,
};
use crate::cardgen::regenerate::{self, RegenerationPlan};
use crate::notetypes::rename::{self, FieldIdentity, FieldRename, FieldRenamePlan, RenameFields};
use crate::notetypes::rewrite::{self, Map, Side, Target};
use crate::notetypes::{AggregateDigest, NotetypeKind, names, new_field, new_template, read, validate, write};
use crate::{CancelToken, EngineError, OpSpec, ProgressSink, SchemaReason};

/// Notes read per page.
const PAGE: usize = 500;

/// What the write phase applies.
#[derive(Clone, Debug)]
pub(crate) enum Planned {
    /// A draft whose only changes are field renames, handed to the field
    /// rename op (S1, decided 2026-10-27): the same result, journal and step.
    Rename(Box<FieldRenamePlan>),
    Layout(Box<Layout>),
}

impl Planned {
    pub(crate) fn write_spec(&self, spec: &OpSpec) -> OpSpec {
        match self {
            Planned::Rename(plan) => write_spec(spec, &[], plan.regenerates()),
            Planned::Layout(layout) => write_spec(spec, &layout.reasons, layout.touches_notes()),
        }
    }
}

/// A planned structure change, shared by the draft apply op and Restore to
/// Default (`restore-to-default` D4).
#[derive(Clone, Debug)]
pub(crate) struct Layout {
    pub base: Notetype,
    /// The digest of `base`: checked again inside the write transaction.
    pub digest: AggregateDigest,
    /// The note type the save stores; the ids of `fresh` parts are given at
    /// commit from the op's random source (R1875).
    pub saved: Notetype,
    pub fresh_fields: Vec<bool>,
    pub fresh_templates: Vec<bool>,
    pub field_origins: Vec<Option<u32>>,
    pub template_origins: Vec<Option<u32>>,
    /// G2's reasons, in G2's order.
    pub reasons: Vec<SchemaReason>,
    pub structure_reasons: BTreeSet<StructureReason>,
    /// Whether the save reports a `layouts` entry (S9).
    pub report_layout: bool,
    /// S5 runs.
    pub rewrite_notes: bool,
    /// The field at the final sort position is not the base's sort field
    /// (S2): reported in D2's summary whatever the schema reasons.
    pub sort_changed: bool,
    pub cards: CardMap,
    /// S7 runs.
    pub fronts_changed: bool,
    /// Base field ordinals the save removes, ascending.
    pub removed_fields: Vec<u32>,
    pub generation: Option<RegenerationPlan>,
    /// Whether the save writes anything (N3.1).
    pub changed: bool,
    pub confirmed: ConfirmedLosses,
    /// What D2 lists that the rows alone do not tell.
    pub text_or_styling_changed: bool,
    /// The report's template map when it is not the origins
    /// (`restore-to-default` D4.1: a cloze note type's card ordinals).
    pub report_template_map: Option<Vec<i32>>,
    /// Which read `LossesChanged` carries.
    pub purpose: Purpose,
    /// The note type name after N4 steps 1 and 2, before step 7's `+`:
    /// the write phase applies step 7 again on the live rows.
    pub requested_name: String,
}

/// The op a layout belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Purpose {
    /// The draft save: D2's summary.
    Draft,
    /// Restore to Default: a `restore_summary` (D7.1).
    Restore,
}

impl Layout {
    pub fn touches_notes(&self) -> bool {
        self.rewrite_notes || !self.cards.is_identity() || self.generation.is_some()
    }

    /// The structure report the save gives (S9, `engine/change-stream`
    /// S2.5), or `None` when it reports nothing.
    pub fn report(&self) -> Result<Option<StructureReport>, EngineError> {
        if !self.report_layout && self.structure_reasons.is_empty() {
            return Ok(None);
        }
        let mut report = StructureReport::default();
        report.reasons.clone_from(&self.structure_reasons);
        if self.report_layout {
            let map = |origins: &[Option<u32>]| -> Vec<i32> {
                origins
                    .iter()
                    .map(|origin| origin.map_or(-1, |o| i32::try_from(o).unwrap_or(-1)))
                    .collect()
            };
            report.layouts.insert(
                self.saved.id.0,
                LayoutReport {
                    before: LayoutShape::of(&self.base)?,
                    after: LayoutShape::of(&self.saved)?,
                    field_map: map(&self.field_origins),
                    template_map: self
                        .report_template_map
                        .clone()
                        .unwrap_or_else(|| map(&self.template_origins)),
                },
            );
        }
        Ok(Some(report))
    }
}

/// What S6 (and Restore's D4.1) does to a card of the type, by its `ord`.
#[derive(Clone, Debug, Default)]
pub(crate) struct CardMap {
    /// S6 runs: the templates changed.
    pub templates_changed: bool,
    /// For each base template position, its position after the save, or
    /// `None` when it is removed.
    pub moved: Vec<Option<u16>>,
    /// `restore-to-default` D4.1: cards at or past this `ord` are removed.
    pub remove_from: Option<u16>,
}

/// What happens to a card.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CardOutcome {
    Keep,
    Move(u16),
    Remove,
}

impl CardMap {
    /// The loss category of a card this map removes (D8): its base
    /// template, or `None` for D4.1's kind-change removals.
    pub fn removed_template(&self, ord: u16) -> Option<u32> {
        (self.templates_changed && matches!(self.moved.get(usize::from(ord)), Some(None))).then_some(u32::from(ord))
    }

    pub fn is_identity(&self) -> bool {
        self.remove_from.is_none()
            && (!self.templates_changed
                || self
                    .moved
                    .iter()
                    .enumerate()
                    .all(|(at, to)| to.is_some_and(|to| usize::from(to) == at)))
    }

    pub fn outcome_of(&self, ord: u16) -> CardOutcome {
        let mut target = ord;
        if self.templates_changed {
            // S6 step 3: cards past the base's last template are left alone.
            match self.moved.get(usize::from(ord)) {
                Some(None) => return CardOutcome::Remove,
                Some(Some(to)) => target = *to,
                None => {}
            }
        }
        if self.remove_from.is_some_and(|from| target >= from) {
            return CardOutcome::Remove;
        }
        if target == ord { CardOutcome::Keep } else { CardOutcome::Move(target) }
    }
}

/// The losses and card effects counted on one set of rows.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Counts {
    /// Per removed base field ordinal: notes whose value there is not empty
    /// (E1).
    pub fields: BTreeMap<u32, u64>,
    /// Per removed base template ordinal: its cards.
    pub templates: BTreeMap<u32, u64>,
    /// D4.1's kind-change removals.
    pub kind_change_cards: u64,
    /// Notes with a card before and none after (shown, never compared).
    pub notes_without_cards: u64,
    pub cards_renumbered: u64,
}

impl Counts {
    /// D8: whether any live count is larger than its own confirmed count
    /// (an absent category was confirmed as 0). Never a total.
    pub fn exceed(&self, confirmed: &ConfirmedLosses) -> bool {
        let over = |live: &BTreeMap<u32, u64>, shown: &BTreeMap<u32, u64>| {
            live.iter().any(|(ord, count)| *count > shown.get(ord).copied().unwrap_or(0))
        };
        over(&self.fields, &confirmed.fields)
            || over(&self.templates, &confirmed.templates)
            || self.kind_change_cards > confirmed.kind_change_cards
    }
}

/// One pass over the type's notes on `reader`'s rows, with progress: S1.2
/// item 4 when the save rewrites notes (at most as many values as the base
/// has fields), the notes with content in each removed field, the cards
/// removed per category, the notes left with no card and the cards
/// renumbered. The write phase runs it again on the live rows of its
/// transaction before its first write.
pub(crate) fn count(
    reader: &impl ReadRows,
    layout: &Layout,
    cancel: &CancelToken,
    progress: &ProgressSink,
) -> Result<Counts, EngineError> {
    let mut counts = Counts::default();
    for &field in &layout.removed_fields {
        counts.fields.insert(field, 0);
    }
    for (at, moved) in layout.cards.moved.iter().enumerate() {
        if layout.cards.templates_changed && moved.is_none() {
            counts.templates.insert(position(at), 0);
        }
    }
    let scan_cards = !layout.cards.is_identity();
    if !layout.rewrite_notes && layout.removed_fields.is_empty() && !scan_cards {
        return Ok(counts);
    }
    let base_fields = layout.base.fields.len();
    let total = reader.notetype_note_count(layout.base.id)?;
    progress.report(0, Some(total));
    let mut done = 0;
    let mut after = None;
    loop {
        cancel.check()?;
        let page = reader.notetype_notes_after(layout.base.id, after, PAGE)?;
        let Some(last) = page.last() else { break };
        after = Some(last.id);
        for note in page {
            let values: Vec<&str> = note.field_values().collect();
            if layout.rewrite_notes && values.len() > base_fields {
                return Err(StructureRefusal::NoteFieldCountMismatch { note: note.id }.into());
            }
            for &field in &layout.removed_fields {
                if has_content(&values, field) {
                    *counts.fields.entry(field).or_default() += 1;
                }
            }
            if scan_cards {
                let cards = reader.cards_of_note(note.id)?;
                let mut removed = 0;
                for card in &cards {
                    match layout.cards.outcome_of(card.ord) {
                        CardOutcome::Remove => {
                            removed += 1;
                            match layout.cards.removed_template(card.ord) {
                                Some(template) => *counts.templates.entry(template).or_default() += 1,
                                None => counts.kind_change_cards += 1,
                            }
                        }
                        CardOutcome::Move(_) => counts.cards_renumbered += 1,
                        CardOutcome::Keep => {}
                    }
                }
                if !cards.is_empty() && removed == cards.len() {
                    counts.notes_without_cards += 1;
                }
            }
            done += 1;
            progress.report(done, Some(total));
        }
    }
    Ok(counts)
}

/// Whether a note's value at a stored field ordinal has content: present
/// and not empty by `text/field-emptiness` E1 (so `<br>` alone is empty).
/// The one test of "notes with content" (`notetypes/workspace` W1, W3.1;
/// `notetypes/draft-save` D2; S1.2's confirmed losses).
pub(crate) fn has_content(values: &[&str], ord: u32) -> bool {
    values
        .get(ord as usize)
        .is_some_and(|value| !meguri_text::field_empty::counts_as_empty(value))
}

/// The op's own read of a set of rows (D8): D2's summary, or for a restore
/// a `restore_summary` (D7.1).
pub(crate) fn losses_read(layout: &Layout, counts: &Counts, generation: Option<&RegenerationPlan>) -> Losses {
    let summary = summary::of(layout, counts, generation);
    match layout.purpose {
        Purpose::Draft => Losses::Draft(summary),
        Purpose::Restore => Losses::Restore(RestoreSummary {
            summary,
            kind_change_cards_removed: counts.kind_change_cards,
            digest: layout.digest,
        }),
    }
}

/// `LossesChanged` with [`losses_read`].
pub(crate) fn losses_changed(
    layout: &Layout,
    counts: &Counts,
    generation: Option<&RegenerationPlan>,
) -> EngineError {
    StructureRefusal::LossesChanged(Box::new(losses_read(layout, counts, generation))).into()
}

/// S5: a note's values as the save stores them: for each saved field, the
/// value at its origin (empty for none or past the note's values),
/// normalized as a note save does (N2), with `sfld` and `csum` from the
/// saved sort and first fields (C2 to C4). Stamps are the scope's.
pub(crate) fn remap_note(layout: &Layout, note: &Note, normalize_text: bool) -> Result<Note, EngineError> {
    let values: Vec<&str> = note.field_values().collect();
    if values.len() > layout.base.fields.len() {
        return Err(StructureRefusal::NoteFieldCountMismatch { note: note.id }.into());
    }
    let saved: Vec<String> = layout
        .field_origins
        .iter()
        .map(|origin| {
            let value = origin.and_then(|o| values.get(o as usize).copied()).unwrap_or("");
            meguri_text::normalize::field_as_saved(value, normalize_text).into_owned()
        })
        .collect();
    let mut note = note.clone();
    note.fields = saved.join("\u{1f}");
    note.sort_field =
        meguri_text::checksum::sort_text_of(&saved, layout.saved.config.sort_field_idx as usize).into_owned();
    note.checksum = meguri_text::checksum::note_checksum(saved.first().map_or("", String::as_str));
    Ok(note)
}

/// S6 (and D4.1): a note's cards as the save leaves them, removed cards
/// dropped and moved ones renumbered.
pub(crate) fn remap_cards(layout: &Layout, cards: Vec<Card>) -> Vec<Card> {
    cards
        .into_iter()
        .filter_map(|mut card| match layout.cards.outcome_of(card.ord) {
            CardOutcome::Keep => Some(card),
            CardOutcome::Move(to) => {
                card.ord = to;
                Some(card)
            }
            CardOutcome::Remove => None,
        })
        .collect()
}

/// S7's plan, against the notes and cards as the commit leaves them before
/// it generates: `None` when the fronts did not change.
pub(crate) fn plan_generation(
    reader: &impl ReadRows,
    layout: &Layout,
    cancel: &CancelToken,
    progress: &ProgressSink,
) -> Result<Option<RegenerationPlan>, EngineError> {
    if !layout.fronts_changed {
        return Ok(None);
    }
    let normalize_text = crate::notes::fields::normalize_enabled(reader)?;
    let plan = regenerate::plan_mapped(reader, &layout.saved, cancel, progress, |note, cards| {
        let note = if layout.rewrite_notes {
            remap_note(layout, &note, normalize_text)?
        } else {
            note
        };
        Ok((note, remap_cards(layout, cards)))
    })?;
    Ok(Some(plan))
}

/// Origins that repeat (S1.1): `InvalidArgument`, before every refusal.
fn check_unique(origins: impl Iterator<Item = Option<u32>>, what: &str) -> Result<(), EngineError> {
    let mut seen = HashSet::new();
    for origin in origins.flatten() {
        if !seen.insert(origin) {
            return Err(invalid(format!("the draft repeats {what} origin {origin}")));
        }
    }
    Ok(())
}

fn check_range(origins: impl Iterator<Item = Option<u32>>, len: usize, what: &str) -> Result<(), EngineError> {
    for origin in origins.flatten() {
        if origin as usize >= len {
            return Err(invalid(format!("{what} origin {origin} lies outside the base")));
        }
    }
    Ok(())
}

/// Whether the draft only renames fields (S1, decided 2026-10-27): it is
/// then the field rename op's.
fn rename_only(base: &Notetype, draft: &StructureDraft) -> Option<RenameFields> {
    let identity = |origins: &mut dyn Iterator<Item = Option<u32>>, len: usize| {
        let origins: Vec<_> = origins.collect();
        origins.len() == len && origins.iter().enumerate().all(|(at, o)| *o == Some(position(at)))
    };
    if !identity(&mut draft.fields.iter().map(|f| f.origin), base.fields.len())
        || !identity(&mut draft.templates.iter().map(|t| t.origin), base.templates.len())
        || draft.sort_field.is_some_and(|sort| sort != base.config.sort_field_idx)
        || draft.name != base.name
        || draft.css != base.config.css
    {
        return None;
    }
    if draft
        .fields
        .iter()
        .zip(&base.fields)
        .any(|(d, b)| d.options != super::FieldOptions::of(&b.config))
        || draft
            .templates
            .iter()
            .zip(&base.templates)
            .any(|(d, b)| template_of(Some(b), d).config.to_bytes() != b.config.to_bytes() || d.name != b.name)
    {
        return None;
    }
    let fields: Vec<FieldRename> = draft
        .fields
        .iter()
        .zip(&base.fields)
        .filter(|(d, b)| d.name != b.name)
        .map(|(d, b)| FieldRename {
            field: FieldIdentity::of(b),
            name: d.name.clone(),
        })
        .collect();
    (!fields.is_empty()).then_some(RenameFields {
        notetype: base.id,
        base: draft.base,
        fields,
    })
}

/// A draft template on its base template (S1.1), or on a new one (N5.3:
/// row stamps 0; its id is given at commit).
fn template_of(base: Option<&NotetypeTemplate>, draft: &DraftTemplate) -> NotetypeTemplate {
    let mut template = match base {
        Some(base) => base.clone(),
        None => {
            let mut template = new_template(&draft.name, "", "", &NoRandom);
            template.config.id = None;
            template
        }
    };
    template.name.clone_from(&draft.name);
    template.prior_ord = None;
    let c = &mut template.config;
    draft.front.clone_into(&mut c.q_format);
    draft.back.clone_into(&mut c.a_format);
    draft.browser_front.clone_into(&mut c.q_format_browser);
    draft.browser_back.clone_into(&mut c.a_format_browser);
    c.target_deck_id = draft.deck;
    draft.browser_font_name.clone_into(&mut c.browser_font_name);
    c.browser_font_size = draft.browser_font_size;
    template
}

/// Planning draws no ids: the commit gives fresh parts theirs.
#[derive(Debug)]
pub(crate) struct NoRandom;

impl meguri_model::rng::RandomSource for NoRandom {
    fn next_u64(&self) -> u64 {
        0
    }
}

/// The inputs of [`classify`] beyond the two note types.
pub(crate) struct Shape {
    pub field_origins: Vec<Option<u32>>,
    pub template_origins: Vec<Option<u32>>,
    pub fresh_fields: Vec<bool>,
    pub fresh_templates: Vec<bool>,
    pub confirmed: ConfirmedLosses,
    pub text_or_styling_changed: bool,
    pub purpose: Purpose,
    pub requested_name: String,
}

/// S2: what the draft changes, the card map and the requirements, given
/// the saved note type after normalization (and, for a draft, S4).
pub(crate) fn classify(base: Notetype, saved: Notetype, shape: Shape) -> Layout {
    let identity = |origins: &[Option<u32>], len: usize| {
        origins.len() == len && origins.iter().enumerate().all(|(at, o)| *o == Some(position(at)))
    };
    let fields_changed = !identity(&shape.field_origins, base.fields.len());
    let templates_changed = !identity(&shape.template_origins, base.templates.len());
    let sort_origin = shape
        .field_origins
        .get(saved.config.sort_field_idx as usize)
        .copied()
        .flatten();
    let sort_changed = sort_origin != Some(base.config.sort_field_idx);
    let renamed = |origins: &[Option<u32>], names: &mut dyn Iterator<Item = &str>, base: &dyn Fn(usize) -> String| {
        origins
            .iter()
            .zip(names)
            .any(|(origin, name)| origin.is_some_and(|o| base(o as usize) != name))
    };
    let fields_renamed = renamed(
        &shape.field_origins,
        &mut saved.fields.iter().map(|f| f.name.as_str()),
        &|o| base.fields[o].name.clone(),
    );
    let templates_renamed = renamed(
        &shape.template_origins,
        &mut saved.templates.iter().map(|t| t.name.as_str()),
        &|o| base.templates[o].name.clone(),
    );
    let removed_fields: Vec<u32> = (0..base.fields.len())
        .map(position)
        .filter(|o| !shape.field_origins.contains(&Some(*o)))
        .collect();
    // S2: a template added, even at the same count with an equal front, or
    // the fronts by position differ.
    let fronts_changed = shape.template_origins.iter().any(Option::is_none)
        || saved.templates.len() != base.templates.len()
        || saved
            .templates
            .iter()
            .zip(&base.templates)
            .any(|(a, b)| a.config.q_format != b.config.q_format);
    let mut reasons = Vec::new();
    let mut structure_reasons = BTreeSet::new();
    if fields_changed {
        reasons.push(SchemaReason::FieldsChanged);
        structure_reasons.insert(StructureReason::FieldsChanged);
    }
    if sort_changed {
        reasons.push(SchemaReason::SortFieldChanged);
        structure_reasons.insert(StructureReason::SortFieldChanged);
    }
    if templates_changed {
        reasons.push(SchemaReason::TemplatesChanged);
        structure_reasons.insert(StructureReason::TemplatesChanged);
    }
    let moved = (0..base.templates.len())
        .map(|at| {
            shape
                .template_origins
                .iter()
                .position(|o| *o == Some(position(at)))
                .map(|to| u16::try_from(to).unwrap_or(u16::MAX))
        })
        .collect();
    let digest = AggregateDigest::of(&base);
    // A save whose origins move, add or remove parts writes even when its
    // rows read as the base's (legacy parts without ids and equal options).
    let changed = shape.fresh_fields.iter().chain(&shape.fresh_templates).any(|f| *f)
        || fields_changed
        || templates_changed
        || sort_changed
        || AggregateDigest::of(&saved) != digest;
    Layout {
        report_layout: fields_changed || templates_changed || sort_changed || fields_renamed || templates_renamed,
        rewrite_notes: fields_changed || sort_changed,
        sort_changed,
        cards: CardMap {
            templates_changed,
            moved,
            remove_from: None,
        },
        fronts_changed,
        removed_fields,
        generation: None,
        reasons,
        structure_reasons,
        digest,
        base,
        saved,
        fresh_fields: shape.fresh_fields,
        fresh_templates: shape.fresh_templates,
        field_origins: shape.field_origins,
        template_origins: shape.template_origins,
        changed,
        confirmed: shape.confirmed,
        text_or_styling_changed: shape.text_or_styling_changed,
        report_template_map: None,
        purpose: shape.purpose,
        requested_name: shape.requested_name,
    }
}

/// N6 for the saved templates, keeping the unknown members of each kept
/// template's stored requirement (F3).
pub(crate) fn requirements(base: &Notetype, saved: &Notetype, origins: &[Option<u32>]) -> Vec<CardRequirement> {
    write::card_requirements(saved)
        .into_iter()
        .zip(origins)
        .map(|(computed, origin)| {
            let mut requirement = origin
                .and_then(|o| base.config.reqs.iter().find(|old| old.card_ord == o))
                .cloned()
                .unwrap_or_default();
            requirement.card_ord = computed.card_ord;
            requirement.kind = computed.kind;
            requirement.field_ords = computed.field_ords;
            requirement
        })
        .collect()
}

/// The whole plan of a draft (S3 steps 1 to 5), its refusals in S1.2's
/// order.
pub(crate) fn plan_draft(
    reader: &impl ReadRows,
    draft: &StructureDraft,
    cancel: &CancelToken,
    progress: &ProgressSink,
) -> Result<Planned, EngineError> {
    cancel.check()?;
    let base = checked_base(reader, draft)?;
    if let Some(request) = rename_only(&base, draft) {
        let plan = rename::plan(reader, request, cancel, progress)?;
        // The field rename op skips V3 (the field editor's mode); a checked
        // draft runs it on the renamed note type (V1, S1.2 item 5).
        if !draft.skip_checks
            && let Some(failure) = validate::first_failure(plan.renamed())
        {
            return Err(failure.into());
        }
        return Ok(Planned::Rename(Box::new(plan)));
    }
    let mut layout = layout_of_draft(reader, base, draft)?;
    check_protected(&layout)?;
    finish(reader, &mut layout, !draft.skip_checks, cancel, progress)?;
    Ok(Planned::Layout(Box::new(layout)))
}

/// S1.2's whole order, steps 1 to 3, on `reader`'s rows: the draft-only
/// `InvalidArgument`, then `Stale`, then an origin outside the base. The
/// base, which is then the stored note type.
pub(crate) fn checked_base(reader: &impl ReadRows, draft: &StructureDraft) -> Result<Notetype, EngineError> {
    // Step 1: what the draft alone shows (S1.1).
    check_unique(draft.fields.iter().map(|f| f.origin), "a field")?;
    check_unique(draft.templates.iter().map(|t| t.origin), "a template")?;
    if draft.sort_field.is_some_and(|sort| sort as usize >= draft.fields.len()) {
        return Err(invalid("the sort field is not a field of the draft"));
    }
    let id = draft.notetype;
    // Step 2, S1.2 item 1 on the planning snapshot: a note type that no
    // longer exists is `Stale` too (changed 2026-11-20, round 9 review).
    let base = reader.notetype(id)?.ok_or_else(|| stale(id))?;
    if AggregateDigest::of(&base) != draft.base {
        return Err(stale(id));
    }
    // Step 3: origins outside the base (S1.1). Only now is the base the
    // stored note type, so a draft loaded before another write removed a
    // field is `Stale` above, never `InvalidArgument`.
    check_range(draft.fields.iter().map(|f| f.origin), base.fields.len(), "a field")?;
    check_range(draft.templates.iter().map(|t| t.origin), base.templates.len(), "a template")?;
    Ok(base)
}

/// S1.2 item 3: a removed field that its base protects.
fn check_protected(layout: &Layout) -> Result<(), EngineError> {
    match layout
        .removed_fields
        .iter()
        .find(|&&o| layout.base.fields[o as usize].config.prevent_deletion)
    {
        Some(&field) => Err(StructureRefusal::ProtectedField { field }.into()),
        None => Ok(()),
    }
}

/// `notetypes/draft-save` D2's `draft_summary` on `reader`'s rows: the
/// planner's own layout of the draft and its own counting pass ([`count`],
/// [`plan_generation`]), so the numbers the sheet shows are the ones the
/// save counts again (S1.2) and the summary `LossesChanged` would carry on
/// the same rows. It refuses as planning does before its loss comparison
/// (`InvalidArgument`, `Stale`, `invalid_notetype`, `ProtectedField`,
/// `NoteFieldCountMismatch`, `Cancelled`), runs no template checks (those
/// are the live issues, `notetypes/workspace` W7) and compares no
/// confirmed loss. A rename-only draft, which the save hands to the field
/// rename op, lists its renames.
pub(crate) fn summarize_draft(
    reader: &impl ReadRows,
    draft: &StructureDraft,
    cancel: &CancelToken,
    progress: &ProgressSink,
) -> Result<super::DraftSummary, EngineError> {
    cancel.check()?;
    let base = checked_base(reader, draft)?;
    let layout = layout_of_draft(reader, base, draft)?;
    check_protected(&layout)?;
    let counts = count(reader, &layout, cancel, progress)?;
    // The generation plan gives D2 only the cards of added card types: a
    // draft without one gives the same summary without it (S7's pass is
    // the save's).
    let generation = if layout.template_origins.iter().any(Option::is_none) {
        plan_generation(reader, &layout, cancel, progress)?
    } else {
        None
    };
    cancel.check()?;
    Ok(summary::of(&layout, &counts, generation.as_ref()))
}

/// S1.2 items 4 and 5, the generation plan and the confirmed losses, on
/// the planning snapshot: a confirmation already out of date there is
/// refused before the safety backup (D8). Shared with Restore to Default,
/// whose restore is a checked save with no template refusal
/// (`restore-to-default` D3).
pub(crate) fn finish(
    reader: &impl ReadRows,
    layout: &mut Layout,
    checked: bool,
    cancel: &CancelToken,
    progress: &ProgressSink,
) -> Result<(), EngineError> {
    if !layout.changed {
        // N3: a checked save validates before it compares.
        if checked && let Some(failure) = validate::first_failure(&layout.saved) {
            return Err(failure.into());
        }
        return Ok(());
    }
    // Item 4, with the loss counts in the same pass.
    let counts = count(reader, layout, cancel, progress)?;
    // Item 5.
    if checked && let Some(failure) = validate::first_failure(&layout.saved) {
        return Err(failure.into());
    }
    layout.generation = plan_generation(reader, layout, cancel, progress)?;
    cancel.check()?;
    if counts.exceed(&layout.confirmed) {
        return Err(losses_changed(layout, &counts, layout.generation.as_ref()));
    }
    Ok(())
}

/// S3 steps 1 to 4 for a draft: the saved note type built on the base,
/// normalized (N4), rewritten (S4), with its sort field and requirements.
pub(crate) fn layout_of_draft(
    reader: &impl ReadRows,
    base: Notetype,
    draft: &StructureDraft,
) -> Result<Layout, EngineError> {
    let mut saved = base.clone();
    draft.name.clone_into(&mut saved.name);
    draft.css.clone_into(&mut saved.config.css);
    saved.fields = draft
        .fields
        .iter()
        .map(|d| {
            let mut field = match d.origin {
                Some(o) => base.fields[o as usize].clone(),
                None => {
                    let mut field = new_field(&d.name, &NoRandom);
                    field.config.id = None;
                    field
                }
            };
            d.name.clone_into(&mut field.name);
            d.options.apply(&mut field.config);
            // N4 step 6 follows the base's sort field by this.
            field.prior_ord = d.origin;
            field
        })
        .collect::<Vec<NotetypeField>>();
    saved.templates = draft
        .templates
        .iter()
        .map(|d| template_of(d.origin.map(|o| &base.templates[o as usize]), d))
        .collect();
    let others: Vec<(NotetypeId, String)> = read::names(reader)?
        .into_iter()
        .filter(|(other, _)| *other != base.id)
        .collect();
    // S3 step 1 and S1.2 item 2 (V2's counts and the names N4 refuses);
    // the name before step 7's `+` is kept for the write phase.
    names::normalize(&mut saved, &others)?;
    let requested_name = names::requested_name(&draft.name)?;
    // S3 step 4: a sort field the draft names, at its final position.
    if let Some(sort) = draft.sort_field {
        saved.config.sort_field_idx = sort;
    }
    for field in &mut saved.fields {
        field.prior_ord = None;
    }
    let field_origins: Vec<Option<u32>> = draft.fields.iter().map(|f| f.origin).collect();
    let template_origins: Vec<Option<u32>> = draft.templates.iter().map(|t| t.origin).collect();
    // S3 step 2 (S4): renamed and removed fields' references.
    let mut map_entries: Vec<(String, Option<String>)> = Vec::new();
    for (field, origin) in saved.fields.iter().zip(&field_origins) {
        if let Some(o) = origin {
            let old = &base.fields[*o as usize].name;
            if *old != field.name {
                map_entries.push((old.clone(), Some(field.name.clone())));
            }
        }
    }
    for (at, field) in base.fields.iter().enumerate() {
        if !field_origins.contains(&Some(position(at))) {
            map_entries.push((field.name.clone(), None));
        }
    }
    if !map_entries.is_empty() {
        let map: Map<'_> = map_entries
            .iter()
            .map(|(old, new)| {
                let target = new.as_deref().map_or(Target::Removed, Target::Renamed);
                (old.as_str(), target)
            })
            .collect();
        let cloze = NotetypeKind::of(&saved) == NotetypeKind::Cloze;
        let first = saved.fields[0].name.clone();
        for template in &mut saved.templates {
            let c = &mut template.config;
            c.q_format = rewrite::rewrite(&c.q_format, &map, &first, cloze, Side::Front);
            c.a_format = rewrite::rewrite(&c.a_format, &map, &first, cloze, Side::Back);
            c.q_format_browser = rewrite::rewrite(&c.q_format_browser, &map, &first, cloze, Side::BrowserFront);
            c.a_format_browser = rewrite::rewrite(&c.a_format_browser, &map, &first, cloze, Side::BrowserBack);
        }
    }
    // S3 step 3 (S8): requirements from the templates as S4 left them, also
    // when a field or template was replaced by one of the same name (the
    // origins changed while names and fronts read the same).
    let origins_changed = |origins: &[Option<u32>], len: usize| {
        origins.len() != len || origins.iter().enumerate().any(|(at, o)| *o != Some(position(at)))
    };
    if write::requirements_stale(&base, &saved)
        || origins_changed(&field_origins, base.fields.len())
        || origins_changed(&template_origins, base.templates.len())
    {
        saved.config.reqs = requirements(&base, &saved, &template_origins);
    }
    let text_or_styling_changed = draft.css != base.config.css
        || draft.templates.iter().any(|d| {
            d.origin.is_some_and(|o| {
                let c = &base.templates[o as usize].config;
                d.front != c.q_format
                    || d.back != c.a_format
                    || d.browser_front != c.q_format_browser
                    || d.browser_back != c.a_format_browser
            })
        });
    let shape = Shape {
        confirmed: draft.confirmed.clone(),
        text_or_styling_changed,
        purpose: Purpose::Draft,
        requested_name,
        fresh_fields: field_origins.iter().map(Option::is_none).collect(),
        fresh_templates: template_origins.iter().map(Option::is_none).collect(),
        field_origins,
        template_origins,
    };
    Ok(classify(base, saved, shape))
}
