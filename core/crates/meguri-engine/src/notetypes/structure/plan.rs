//! Planning a structure change on a snapshot (S2, S3 steps 1 to 5, S10):
//! the saved note type, what changed, the rewritten templates (S4), the
//! note remap (S5), the card map (S6), the generation plan (S7) and the
//! refusals of S1.2 in their order. The same scans run again on the live
//! rows in the write phase ([`super::commit`]).

use std::collections::{BTreeSet, HashSet};

use meguri_model::card::Card;
use meguri_model::ids::NotetypeId;
use meguri_model::note::Note;
use meguri_model::notetype::{Notetype, NotetypeField, NotetypeTemplate};
use meguri_model::proto::notetype::CardRequirement;
use meguri_storage::ReadRows;
use meguri_storage::journal::{LayoutReport, LayoutShape, StructureReason, StructureReport};

use super::{DraftTemplate, StructureDraft, StructureRefusal, invalid, position, stale, write_spec};
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
    pub cards: CardMap,
    /// S7 runs.
    pub fronts_changed: bool,
    /// Base field ordinals the save removes, ascending.
    pub removed_fields: Vec<u32>,
    pub generation: Option<RegenerationPlan>,
    /// Whether the save writes anything (N3.1).
    pub changed: bool,
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
                    template_map: map(&self.template_origins),
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

/// S1.2 item 4 on `reader`'s rows: when the save rewrites notes, every note
/// of the type must hold at most as many values as the base has fields.
/// One pass over the type's notes, with progress.
///
/// The write phase runs it again on the live rows of its transaction
/// before its first write; any further precondition on the live rows
/// belongs beside it, after the base token check (`commit::check_live`).
pub(crate) fn check_note_values(
    reader: &impl ReadRows,
    layout: &Layout,
    cancel: &CancelToken,
    progress: &ProgressSink,
) -> Result<(), EngineError> {
    if !layout.rewrite_notes {
        return Ok(());
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
            if note.field_values().count() > base_fields {
                return Err(StructureRefusal::NoteFieldCountMismatch { note: note.id }.into());
            }
            done += 1;
            progress.report(done, Some(total));
        }
    }
    Ok(())
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
    let fronts_changed = saved.templates.len() != base.templates.len()
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
    let changed = shape.fresh_fields.iter().chain(&shape.fresh_templates).any(|f| *f)
        || AggregateDigest::of(&saved) != digest;
    Layout {
        report_layout: fields_changed || templates_changed || sort_changed || fields_renamed || templates_renamed,
        rewrite_notes: fields_changed || sort_changed,
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
    check_unique(draft.fields.iter().map(|f| f.origin), "a field")?;
    check_unique(draft.templates.iter().map(|t| t.origin), "a template")?;
    let id = draft.notetype;
    let base = reader
        .notetype(id)?
        .ok_or_else(|| invalid(format!("no note type {id}")))?;
    // S1.2 item 1, on the planning snapshot.
    if AggregateDigest::of(&base) != draft.base {
        return Err(stale(id));
    }
    check_range(draft.fields.iter().map(|f| f.origin), base.fields.len(), "a field")?;
    check_range(draft.templates.iter().map(|t| t.origin), base.templates.len(), "a template")?;
    if draft.sort_field.is_some_and(|sort| sort as usize >= draft.fields.len()) {
        return Err(invalid("the sort field is not a field of the draft"));
    }
    if let Some(request) = rename_only(&base, draft) {
        let plan = rename::plan(reader, request, cancel, progress)?;
        return Ok(Planned::Rename(Box::new(plan)));
    }
    let mut layout = layout_of_draft(reader, base, draft)?;
    // S1.2 item 3.
    if let Some(&field) = layout
        .removed_fields
        .iter()
        .find(|&&o| layout.base.fields[o as usize].config.prevent_deletion)
    {
        return Err(StructureRefusal::ProtectedField { field }.into());
    }
    finish(reader, &mut layout, !draft.skip_checks, cancel, progress)?;
    Ok(Planned::Layout(Box::new(layout)))
}

/// S1.2 items 4 and 5 and the generation plan, on the planning snapshot.
/// Shared with Restore to Default, whose restore is a checked save with no
/// template refusal (`restore-to-default` D3).
pub(crate) fn finish(
    reader: &impl ReadRows,
    layout: &mut Layout,
    checked: bool,
    cancel: &CancelToken,
    progress: &ProgressSink,
) -> Result<(), EngineError> {
    if !layout.changed {
        return Ok(());
    }
    // Item 4.
    check_note_values(reader, layout, cancel, progress)?;
    // Item 5.
    if checked && let Some(failure) = validate::first_failure(&layout.saved) {
        return Err(failure.into());
    }
    layout.generation = plan_generation(reader, layout, cancel, progress)?;
    cancel.check()?;
    Ok(())
}

/// S3 steps 1 to 4 for a draft: the saved note type built on the base,
/// normalized (N4), rewritten (S4), with its sort field and requirements.
fn layout_of_draft(reader: &impl ReadRows, base: Notetype, draft: &StructureDraft) -> Result<Layout, EngineError> {
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
    // S3 step 1 and S1.2 item 2 (V2's counts and the names N4 refuses).
    names::normalize(&mut saved, &others)?;
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
    // S3 step 3 (S8): requirements from the templates as S4 left them.
    if write::requirements_stale(&base, &saved) {
        saved.config.reqs = requirements(&base, &saved, &template_origins);
    }
    let shape = Shape {
        fresh_fields: field_origins.iter().map(Option::is_none).collect(),
        fresh_templates: template_origins.iter().map(Option::is_none).collect(),
        field_origins,
        template_origins,
    };
    Ok(classify(base, saved, shape))
}
