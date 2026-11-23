//! Merge note types (`package/notetype-merge`, the port of
//! `package/apkg-import` I8): stage 1 merges each incoming note type with
//! the target's note type of its id and that one's earlier copies; stage 2
//! brings together the note types of notes both sides hold. Planning only:
//! the records, moves and maps computed here are written by `commit`.
use std::collections::{BTreeMap, HashMap, HashSet};

use meguri_engine::cardgen::regenerate;
use meguri_engine::notetypes::NotetypeKind;
use meguri_engine::notetypes::import as saves;
use meguri_engine::{CancelToken, ProgressSink, SchemaReason};
use meguri_model::ids::{NoteId, NotetypeId};
use meguri_model::notetype::Notetype;
use meguri_storage::journal::{LayoutReport, LayoutShape, StructureReason, TypeChangeReport};
use meguri_storage::{NoteIdentity, ReadRows};

use super::gather::RawPackage;
use super::notetypes::{TypeRoute, TypeStage, TypeTarget, TypeWrite, fronts, normalized, refused};
use super::{
    ApkgImportError, DroppedValues, EmptiedField, ImportOptions, ImportOutcome, RemovedNotetype, UpdateCondition,
};

/// What J3 compares: an id, when the record stores one, and the name.
type Key<'a> = (Option<i64>, &'a str);

fn same(a: Key<'_>, b: Key<'_>) -> bool {
    match (a.0, b.0) {
        (Some(a), Some(b)) => a == b,
        _ => a.1 == b.1,
    }
}

/// J3's one-to-one matching of `record` against a list that starts as
/// `list` and gains the record's new entries at its end: for each entry of
/// the record, its position in the list.
fn align<'a>(list: &mut Vec<Key<'a>>, record: &[Key<'a>]) -> Vec<usize> {
    let known = list.len();
    let mut taken = vec![false; known];
    let mut positions = Vec::with_capacity(record.len());
    for entry in record {
        let found = (0..known).find(|at| !taken[*at] && same(list[*at], *entry));
        match found {
            Some(at) => {
                taken[at] = true;
                positions.push(at);
            }
            None => {
                positions.push(list.len());
                list.push(*entry);
            }
        }
    }
    positions
}

fn field_keys(notetype: &Notetype) -> Vec<Key<'_>> {
    notetype.fields.iter().map(|f| (f.config.id, f.name.as_str())).collect()
}

fn template_keys(notetype: &Notetype) -> Vec<Key<'_>> {
    notetype.templates.iter().map(|t| (t.config.id, t.name.as_str())).collect()
}

/// For each position below `len`, the entry of the record placed there.
fn inverse(positions: &[usize], len: usize) -> Vec<Option<usize>> {
    let mut out = vec![None; len];
    for (from, to) in positions.iter().enumerate() {
        if let Some(slot) = out.get_mut(*to) {
            *slot = Some(from);
        }
    }
    out
}

fn as_u32(map: &[Option<usize>]) -> Vec<Option<u32>> {
    map.iter().map(|entry| entry.and_then(|at| u32::try_from(at).ok())).collect()
}

fn is_cloze(notetype: &Notetype) -> bool {
    NotetypeKind::of(notetype) == NotetypeKind::Cloze
}

fn governs(rule: UpdateCondition, target: &Notetype, incoming: &Notetype) -> bool {
    match rule {
        UpdateCondition::IfNewer => target.mtime.0 < incoming.mtime.0,
        UpdateCondition::Always => true,
        UpdateCondition::Never => false,
    }
}

/// One move of notes between two note types, kept until the final layouts
/// are known for the structure report.
struct Pair {
    old: i64,
    new: i64,
    /// Per position of the new note type (as it was then): the old position.
    fields: Vec<Option<usize>>,
    templates: Option<Vec<Option<usize>>>,
}

/// How a target note's stored values reach its note type's final layout:
/// for each final position, the stored position or none.
type Origins = Vec<Option<usize>>;

fn compose(first: &Origins, then: &[Option<usize>]) -> Origins {
    then.iter().map(|at| at.and_then(|at| first.get(at).copied().flatten())).collect()
}

/// The number of notes of `notetype` that store more values than `fields`.
fn count_extra(reader: &impl ReadRows, notetype: i64, fields: usize, cancel: &CancelToken) -> ImportOutcome<u64> {
    let mut count = 0;
    let mut after = None;
    loop {
        cancel.check()?;
        let page = reader.notetype_notes_after(NotetypeId(notetype), after, 500)?;
        let Some(last) = page.last() else { break };
        after = Some(last.id);
        count += page.iter().filter(|note| note.field_values().count() > fields).count() as u64;
    }
    Ok(count)
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn plan(
    reader: &impl ReadRows,
    raw: &RawPackage,
    taken: &[Notetype],
    targets: &BTreeMap<i64, Notetype>,
    matches: &HashMap<&[u8], NoteIdentity>,
    options: &ImportOptions,
    now_s: i64,
    cancel: &CancelToken,
    progress: &ProgressSink,
) -> ImportOutcome<TypeStage> {
    let mut stage = TypeStage::default();
    // The target's note types as the pass leaves them.
    let mut state: BTreeMap<i64, Notetype> = targets.clone();
    let incoming_ids: HashSet<i64> = taken.iter().map(|nt| nt.id.0).collect();
    // Names of note types the pass removes stay taken: their rows go only
    // after the notes are written (J8), later than every note type write.
    let leaving: std::cell::RefCell<Vec<(NotetypeId, String)>> = std::cell::RefCell::new(Vec::new());
    let names = |state: &BTreeMap<i64, Notetype>| -> Vec<(NotetypeId, String)> {
        let mut names: Vec<(NotetypeId, String)> = state.values().map(|nt| (nt.id, nt.name.clone())).collect();
        names.extend(leaving.borrow().iter().cloned());
        names
    };
    let mut pairs: Vec<Pair> = Vec::new();
    let mut effect = false;
    // By snapshot `mid`: how the notes of that note type reach the layout
    // stage 1 leaves them on.
    let mut layouts: HashMap<i64, Origins> = HashMap::new();
    let mut dropped: BTreeMap<i64, u64> = BTreeMap::new();
    let mut removed: BTreeMap<i64, RemovedNotetype> = BTreeMap::new();

    // Stage 1 (J5): one ascending pass.
    for incoming in taken {
        cancel.check()?;
        let id = incoming.id.0;
        let Some(target) = targets.get(&id) else {
            let mut record = incoming.clone();
            let stamp_now = normalized(&mut record, id, &names(&state))?;
            state.insert(id, record.clone());
            stage.routes.insert(
                id,
                TypeRoute {
                    target: TypeTarget::Id(id),
                    record: record.clone(),
                    field_map: None,
                    template_map: None,
                },
            );
            stage.writes.push(TypeWrite::Put { record, stamp_now });
            stage.added += 1;
            continue;
        };
        if is_cloze(target) != is_cloze(incoming) {
            return Err(ApkgImportError::NotetypeKindConflict { incoming: id, target: id });
        }
        let cloze = is_cloze(target);
        let copies: Vec<&Notetype> = targets
            .values()
            .filter(|other| {
                other.id.0 != id
                    && other.config.original_id == Some(id)
                    && !incoming_ids.contains(&other.id.0)
                    && is_cloze(other) == cloze
            })
            .collect();
        // J4.1: T's list, then each copy, then the incoming note type.
        let mut field_list = field_keys(target);
        let copy_fields: Vec<Vec<usize>> = copies.iter().map(|copy| align(&mut field_list, &field_keys(copy))).collect();
        let incoming_fields = align(&mut field_list, &field_keys(incoming));
        let field_count = field_list.len();
        let mut template_list = template_keys(target);
        let (copy_templates, incoming_templates): (Vec<Vec<usize>>, Vec<usize>) = if cloze {
            (Vec::new(), Vec::new())
        } else {
            let copies = copies
                .iter()
                .map(|copy| align(&mut template_list, &template_keys(copy)))
                .collect();
            (copies, align(&mut template_list, &template_keys(incoming)))
        };
        let template_count = template_list.len();

        // J4.2, J4.3: the governing record's values, then the first record
        // that has the entry, in the order T, copies, incoming.
        let incoming_governs = governs(options.update_notetypes, target, incoming);
        let governing = if incoming_governs { incoming } else { target };
        let mut merged = governing.clone();
        merged.id = target.id;
        let target_fields: Vec<usize> = (0..target.fields.len()).collect();
        let mut sources: Vec<(&Notetype, &[usize])> = Vec::new();
        if incoming_governs {
            sources.push((incoming, &incoming_fields));
        }
        sources.push((target, &target_fields));
        for (copy, positions) in copies.iter().zip(&copy_fields) {
            sources.push((copy, positions));
        }
        sources.push((incoming, &incoming_fields));
        merged.fields = (0..field_count)
            .filter_map(|at| {
                sources.iter().find_map(|(record, positions)| {
                    positions.iter().position(|to| *to == at).map(|from| record.fields[from].clone())
                })
            })
            .collect();
        if !cloze {
            let target_templates: Vec<usize> = (0..target.templates.len()).collect();
            let mut sources: Vec<(&Notetype, &[usize])> = Vec::new();
            if incoming_governs {
                sources.push((incoming, &incoming_templates));
            }
            sources.push((target, &target_templates));
            for (copy, positions) in copies.iter().zip(&copy_templates) {
                sources.push((copy, positions));
            }
            sources.push((incoming, &incoming_templates));
            merged.templates = (0..template_count)
                .filter_map(|at| {
                    sources.iter().find_map(|(record, positions)| {
                        positions.iter().position(|to| *to == at).map(|from| record.templates[from].clone())
                    })
                })
                .collect();
        }
        // J4.4, J4.5.
        let others: Vec<(NotetypeId, String)> = names(&state);
        saves::normalize_merged(&mut merged, &others, governing.config.sort_field_idx)
            .map_err(|reason| refused(id, reason))?;
        let renames: Vec<(String, String)> = target
            .fields
            .iter()
            .zip(&merged.fields)
            .filter(|(before, after)| before.name != after.name)
            .map(|(before, after)| (before.name.clone(), after.name.clone()))
            .collect();
        saves::rewrite_renamed_fields(&mut merged, &renames);

        // Step 3: T's notes.
        let grew = merged.fields.len() > target.fields.len();
        let sort_moved = merged.config.sort_field_idx != target.config.sort_field_idx;
        let stored: Origins = (0..merged.fields.len())
            .map(|at| (at < target.fields.len()).then_some(at))
            .collect();
        if grew || sort_moved {
            stage.writes.push(TypeWrite::RewriteNotes {
                notetype: id,
                record: merged.clone(),
                origins: stored.clone(),
            });
            stage.stamped.insert(id);
            stage.rows += reader.notetype_note_count(target.id)?;
            let extra = count_extra(reader, id, target.fields.len(), cancel)?;
            if extra > 0 {
                *dropped.entry(id).or_default() += extra;
            }
            effect = true;
        }
        layouts.insert(id, stored);
        stage.writes.push(TypeWrite::Put {
            record: merged.clone(),
            stamp_now: true,
        });
        // Step 4: T's cards.
        let more_templates = merged.templates.len() > target.templates.len();
        if more_templates || fronts(&merged) != fronts(target) {
            let planned = regenerate::plan(reader, &merged, cancel, progress)?;
            stage.rows += planned.notes.len() as u64;
            stage.writes.push(TypeWrite::Regenerate(Box::new(planned)));
        }
        effect |= more_templates;
        // Step 5: the copies' notes.
        for (index, copy) in copies.iter().enumerate() {
            let fields = inverse(&copy_fields[index], merged.fields.len());
            let templates = (!cloze).then(|| inverse(&copy_templates[index], merged.templates.len()));
            stage.writes.push(TypeWrite::MoveNotes {
                from: copy.id.0,
                record: merged.clone(),
                fields: as_u32(&fields),
                templates: templates.as_deref().map(as_u32),
                notes: None,
            });
            let notes = reader.notetype_note_count(copy.id)?;
            let extra = count_extra(reader, copy.id.0, copy.fields.len(), cancel)?;
            if extra > 0 {
                *dropped.entry(copy.id.0).or_default() += extra;
            }
            removed.insert(
                copy.id.0,
                RemovedNotetype {
                    notetype: copy.id.0,
                    name: copy.name.clone(),
                    moved_to: id,
                    notes,
                },
            );
            stage.merged_into.insert(copy.id.0, id);
            stage.stamped.insert(copy.id.0);
            stage.removals.push(copy.id.0);
            layouts.insert(copy.id.0, fields.clone());
            pairs.push(Pair {
                old: copy.id.0,
                new: id,
                fields,
                templates,
            });
            leaving.borrow_mut().push((copy.id, copy.name.clone()));
            state.remove(&copy.id.0);
            stage.rows += notes;
            effect = true;
        }
        // J7: the maps of the incoming note type.
        stage.routes.insert(
            id,
            TypeRoute {
                target: TypeTarget::Id(id),
                record: merged.clone(),
                field_map: Some(inverse(&incoming_fields, merged.fields.len())),
                template_map: (!cloze)
                    .then(|| incoming_templates.iter().map(|to| u16::try_from(*to).unwrap_or(u16::MAX)).collect()),
            },
        );
        state.insert(id, merged);
        stage.updated += 1;
    }

    // Stage 2 (J6): notes whose note type differs, by (I, E).
    let mut seen: HashSet<&[u8]> = HashSet::new();
    let mut joins: BTreeMap<(i64, i64), Vec<NoteId>> = BTreeMap::new();
    for note in &raw.notes {
        let mid = note.notetype_id.0;
        if !stage.routes.contains_key(&mid) || !seen.insert(note.guid.as_bytes()) {
            continue;
        }
        if let Some(target) = matches.get(note.guid.as_bytes()) {
            let theirs = stage.mid_after(target.id, target.notetype.0);
            if theirs != mid {
                joins.entry((mid, theirs)).or_default().push(target.id);
            }
        }
    }
    let mut left: HashMap<i64, u64> = HashMap::new();
    let mut moved_extra: BTreeMap<i64, u64> = BTreeMap::new();
    let mut stage_two: HashMap<NoteId, Origins> = HashMap::new();
    let mut notes_by_id: HashMap<NoteId, i64> = HashMap::new();
    for identity in matches.values() {
        notes_by_id.insert(identity.id, identity.notetype.0);
    }
    for ((into, from), mut notes) in joins {
        cancel.check()?;
        notes.sort();
        let (Some(existing), Some(base)) = (state.get(&from).cloned(), state.get(&into).cloned()) else {
            continue;
        };
        if is_cloze(&existing) != is_cloze(&base) {
            return Err(ApkgImportError::NotetypeKindConflict {
                incoming: into,
                target: from,
            });
        }
        let cloze = is_cloze(&base);
        // Step 2: I gains what only E has; I governs.
        let mut field_list = field_keys(&base);
        let from_fields = align(&mut field_list, &field_keys(&existing));
        let mut template_list = template_keys(&base);
        let from_templates = if cloze {
            Vec::new()
        } else {
            align(&mut template_list, &template_keys(&existing))
        };
        let mut grown = base.clone();
        for (at, to) in from_fields.iter().enumerate() {
            if *to >= base.fields.len() {
                grown.fields.push(existing.fields[at].clone());
            }
        }
        for (at, to) in from_templates.iter().enumerate() {
            if *to >= base.templates.len() {
                grown.templates.push(existing.templates[at].clone());
            }
        }
        let others = names(&state);
        saves::normalize_merged(&mut grown, &others, base.config.sort_field_idx).map_err(|reason| refused(into, reason))?;
        // Step 3: I's own notes, by I's own origins.
        if grown.fields.len() > base.fields.len() {
            stage.writes.push(TypeWrite::RewriteNotes {
                notetype: into,
                record: grown.clone(),
                origins: (0..grown.fields.len()).map(|at| (at < base.fields.len()).then_some(at)).collect(),
            });
            if targets.contains_key(&into) {
                stage.rows += reader.notetype_note_count(NotetypeId(into))?;
                let extra = count_extra(reader, into, base.fields.len(), cancel)?;
                if extra > 0 {
                    *dropped.entry(into).or_default() += extra;
                }
            }
        }
        stage.writes.push(TypeWrite::Put {
            record: grown.clone(),
            stamp_now: true,
        });
        if grown.templates.len() > base.templates.len() {
            let planned = regenerate::plan(reader, &grown, cancel, progress)?;
            stage.writes.push(TypeWrite::Regenerate(Box::new(planned)));
        }
        // Step 4: the pair's target notes.
        let fields = inverse(&from_fields, grown.fields.len());
        let templates = (!cloze).then(|| inverse(&from_templates, grown.templates.len()));
        for note in &notes {
            stage.moved.insert(*note, into);
            let snapshot_mid = notes_by_id.get(note).copied().unwrap_or(from);
            let before = layouts
                .get(&snapshot_mid)
                .cloned()
                .unwrap_or_else(|| (0..existing.fields.len()).map(Some).collect());
            stage_two.insert(*note, compose(&before, &fields));
        }
        *left.entry(from).or_default() += notes.len() as u64;
        let moved = notes.len() as u64;
        stage.rows += moved * 2;
        // J10: a moved note that stores more values than its note type has
        // fields loses them, unless stage 1 already counted its note type.
        let mut extra = 0_u64;
        for chunk in notes.chunks(500) {
            for note in reader.notes_with_ids(chunk)? {
                let snapshot_mid = note.notetype_id.0;
                let declared = targets.get(&snapshot_mid).map_or(usize::MAX, |nt| nt.fields.len());
                if !dropped.contains_key(&snapshot_mid) && note.field_values().count() > declared {
                    extra += 1;
                }
            }
        }
        if extra > 0 {
            *moved_extra.entry(from).or_default() += extra;
        }
        stage.writes.push(TypeWrite::MoveNotes {
            from,
            record: grown.clone(),
            fields: as_u32(&fields),
            templates: templates.as_deref().map(as_u32),
            notes: Some(notes),
        });
        removed
            .entry(from)
            .and_modify(|entry| entry.notes += moved)
            .or_insert(RemovedNotetype {
                notetype: from,
                name: existing.name.clone(),
                moved_to: into,
                notes: moved,
            });
        pairs.push(Pair {
            old: from,
            new: into,
            fields,
            templates,
        });
        if let Some(route) = stage.routes.get_mut(&into) {
            let before = route.record.fields.len();
            let mut map = route.field_map.take().unwrap_or_else(|| (0..before).map(Some).collect());
            map.resize(grown.fields.len(), None);
            route.field_map = Some(map);
            route.record = grown.clone();
        }
        state.insert(into, grown);
        effect = true;
    }
    // J8: an E that ends with no target note, and that no package note
    // type keeps in use, goes; one that keeps notes stays as it was.
    let mut gone: Vec<i64> = Vec::new();
    for (from, moved) in &left {
        // Notes of copies merged into `from` in stage 1 count as its notes.
        let mut held = reader.notetype_note_count(NotetypeId(*from))?;
        for (copy, into) in &stage.merged_into {
            if into == from {
                held += reader.notetype_note_count(NotetypeId(*copy))?;
            }
        }
        if held == *moved && !stage.routes.contains_key(from) && state.len() > 1 {
            gone.push(*from);
            state.remove(from);
        }
    }
    gone.sort_unstable();
    for id in &removed.keys().copied().collect::<Vec<_>>() {
        if !stage.removals.contains(id) && !gone.contains(id) {
            removed.remove(id);
        }
    }
    stage.removals.extend(gone);
    stage.removals.sort_unstable();
    for route in stage.routes.values_mut() {
        if let TypeTarget::Id(id) = route.target
            && let Some(record) = state.get(&id)
        {
            route.record = record.clone();
        }
    }

    // J9: the reason, and the structure report against the final layouts.
    for (id, before) in targets {
        let Some(after) = state.get(id) else { continue };
        let changed = after.fields.len() != before.fields.len()
            || after.templates.len() != before.templates.len()
            || after.config.sort_field_idx != before.config.sort_field_idx
            || after.config.kind != before.config.kind
            || super::notetypes::names_differ(before, after);
        if changed {
            let origin = |at: usize, known: usize| if at < known { at as i32 } else { -1 };
            stage.report.layouts.insert(
                *id,
                LayoutReport {
                    before: LayoutShape::of(before)?,
                    after: LayoutShape::of(after)?,
                    field_map: (0..after.fields.len()).map(|at| origin(at, before.fields.len())).collect(),
                    template_map: (0..after.templates.len())
                        .map(|at| origin(at, before.templates.len()))
                        .collect(),
                },
            );
        }
    }
    for pair in &pairs {
        let (Some(before), Some(after)) = (targets.get(&pair.old), state.get(&pair.new)) else {
            continue;
        };
        let stored = |entry: &Option<usize>, known: usize| match entry {
            Some(at) if *at < known => *at as i32,
            _ => -1,
        };
        let mut field_map: Vec<i32> = pair.fields.iter().map(|entry| stored(entry, before.fields.len())).collect();
        field_map.resize(after.fields.len(), -1);
        let template_map = match &pair.templates {
            Some(map) => {
                let mut map: Vec<i32> = map.iter().map(|entry| stored(entry, before.templates.len())).collect();
                map.resize(after.templates.len(), -1);
                map
            }
            None => Vec::new(),
        };
        stage.report.type_changes.insert(
            (pair.old, pair.new),
            TypeChangeReport {
                before: LayoutShape::of(before)?,
                after: LayoutShape::of(after)?,
                field_map,
                template_map,
            },
        );
    }
    stage.report.removed.extend(stage.removals.iter().copied());
    if effect {
        stage.reasons.push(SchemaReason::NotetypesMerged);
        stage.report.reasons.insert(StructureReason::NotetypesMerged);
    }

    // J10: the preview.
    stage.preview.removed_notetypes = removed.into_values().collect();
    for (notetype, notes) in moved_extra {
        *dropped.entry(notetype).or_default() += notes;
    }
    stage.preview.dropped_values = dropped
        .into_iter()
        .map(|(notetype, notes)| DroppedValues { notetype, notes })
        .collect();
    stage.preview.emptied_fields =
        emptied(reader, raw, matches, options, now_s, &stage, &state, targets, &layouts, &stage_two, cancel)?;
    Ok(stage)
}

/// J10's `emptied_fields`: per receiving note type and field position its
/// field map leaves none, the target notes I9 will update that hold text
/// there once the merge ran.
#[allow(clippy::too_many_arguments)]
fn emptied(
    reader: &impl ReadRows,
    raw: &RawPackage,
    matches: &HashMap<&[u8], NoteIdentity>,
    options: &ImportOptions,
    now_s: i64,
    stage: &TypeStage,
    state: &BTreeMap<i64, Notetype>,
    targets: &BTreeMap<i64, Notetype>,
    layouts: &HashMap<i64, Origins>,
    stage_two: &HashMap<NoteId, Origins>,
    cancel: &CancelToken,
) -> ImportOutcome<Vec<EmptiedField>> {
    // Receiving note type to the positions nothing incoming fills.
    let mut open: BTreeMap<i64, Vec<usize>> = BTreeMap::new();
    for (package_type, route) in &stage.routes {
        if let Some(map) = &route.field_map {
            let unfilled: Vec<usize> = map.iter().enumerate().filter(|(_, from)| from.is_none()).map(|(at, _)| at).collect();
            if !unfilled.is_empty() {
                open.insert(*package_type, unfilled);
            }
        }
    }
    if open.is_empty() || options.update_notes == UpdateCondition::Never {
        return Ok(Vec::new());
    }
    // The target notes I9 updates, by receiving note type (J11's `mod`):
    // the same comparison `plan::build` makes.
    let mut seen: HashSet<&[u8]> = HashSet::new();
    let mut updated: BTreeMap<i64, Vec<NoteId>> = BTreeMap::new();
    for note in &raw.notes {
        let mid = note.notetype_id.0;
        if !stage.routes.contains_key(&mid) || !seen.insert(note.guid.as_bytes()) {
            continue;
        }
        let Some(target) = matches.get(note.guid.as_bytes()) else {
            continue;
        };
        if !open.contains_key(&mid) {
            continue;
        }
        let stored = if stage.stamped.contains(&target.notetype.0) { now_s } else { target.mtime };
        let holds = match options.update_notes {
            UpdateCondition::IfNewer => stored < note.mtime.0,
            UpdateCondition::Always => stored != note.mtime.0,
            UpdateCondition::Never => false,
        };
        if holds {
            updated.entry(mid).or_default().push(target.id);
        }
    }
    let by_note: HashMap<NoteId, i64> = matches.values().map(|identity| (identity.id, identity.notetype.0)).collect();
    let mut out = Vec::new();
    for (notetype, positions) in open {
        let Some(record) = state.get(&notetype) else { continue };
        let mut counts = vec![0_u64; positions.len()];
        let ids = updated.remove(&notetype).unwrap_or_default();
        for chunk in ids.chunks(500) {
            cancel.check()?;
            for note in reader.notes_with_ids(chunk)? {
                let values: Vec<&str> = note.field_values().collect();
                let snapshot_mid = by_note.get(&note.id).copied().unwrap_or(notetype);
                let known = targets.get(&snapshot_mid).map_or(values.len(), |nt| nt.fields.len());
                let identity: Origins = (0..known).map(Some).collect();
                let origins = stage_two
                    .get(&note.id)
                    .or_else(|| layouts.get(&snapshot_mid))
                    .unwrap_or(&identity);
                for (slot, position) in positions.iter().enumerate() {
                    let held = origins
                        .get(*position)
                        .copied()
                        .flatten()
                        .filter(|at| *at < known)
                        .and_then(|at| values.get(at));
                    if held.is_some_and(|text| !text.is_empty()) {
                        counts[slot] += 1;
                    }
                }
            }
        }
        for (slot, position) in positions.iter().enumerate() {
            if counts[slot] > 0 {
                out.push(EmptiedField {
                    notetype,
                    notetype_name: record.name.clone(),
                    position: u32::try_from(*position).unwrap_or(u32::MAX),
                    field_name: record.fields.get(*position).map(|f| f.name.clone()).unwrap_or_default(),
                    notes: counts[slot],
                });
            }
        }
    }
    Ok(out)
}
