//! The write phase of an import (`package/apkg-import` I14.1 step 8): one
//! transaction that applies the plan. Whatever answers or deck edits can
//! change while the plan runs is derived again here from the live rows
//! (I14.2): deck matching, id choices, the "pair exists" test, review-log
//! inserts and the day.
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use meguri_engine::cardgen::regenerate;
use meguri_engine::decks::{self, names};
use meguri_engine::{EngineError, OpContext, SchemaReason};
use meguri_model::deck::normalize_native_name;
use meguri_model::grave::GraveKind;
use meguri_model::ids::{CardId, DeckId, NoteId, NotetypeId};
use meguri_model::note::Note;
use meguri_model::notetype::Notetype;
use meguri_model::proto::deck::DeckKind;
use meguri_model::time::EpochSecs;
use meguri_model::usn::Usn;
use meguri_storage::journal::StructureReason;
use meguri_storage::{ConfigValue, ReadRows, Record, RowKey, Table, WriteScope};
use meguri_text::normalize::field_as_saved;

use super::notetypes::{TypeTarget, TypeWrite};
use super::plan::{Decision, ImportPlan};
use super::{ApkgImportError, ImportCounts, ImportLog, ImportOptions, LogEntry};

/// What the write phase hands back.
#[derive(Clone, Debug, Default)]
pub(crate) struct Committed {
    pub log: ImportLog,
    pub counts: ImportCounts,
    /// Static media entries to write once the op committed (I12.4).
    pub statics: Vec<usize>,
}

/// Where a typed refusal of the write phase waits for the caller: the op
/// itself can only return an engine error.
pub(crate) type Fault = Arc<Mutex<Option<ApkgImportError>>>;

fn refuse(fault: &Fault, error: ApkgImportError) -> EngineError {
    *fault.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(error);
    EngineError::InvalidArgument {
        message: "the import was refused in its write phase".into(),
    }
}

fn stale(table: Table, key: impl ToString) -> EngineError {
    EngineError::Stale {
        table,
        key: key.to_string(),
    }
}

/// I13: `text/strip-html` H5, line feeds to spaces, at most 80 bytes.
fn reduced(field: &str) -> String {
    let text = meguri_text::html::strip_markup_keep_media(field).replace('\n', " ");
    let mut end = text.len().min(80);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].to_owned()
}

fn entry(note: i64, fields: &[String], existing: Option<i64>) -> LogEntry {
    LogEntry {
        note,
        fields: fields.iter().map(|field| reduced(field)).collect(),
        existing_id: existing,
    }
}

/// I5: the five keys, each written only when its value differs (C4).
fn write_options(scope: &mut WriteScope<'_>, options: &ImportOptions) -> Result<(), EngineError> {
    scope.set_config("mergeNotetypes", &ConfigValue::from(options.merge_notetypes))?;
    scope.set_config("updateNotes", &ConfigValue::from(options.update_notes.stored()))?;
    scope.set_config("updateNotetypes", &ConfigValue::from(options.update_notetypes.stored()))?;
    scope.set_config("withScheduling", &ConfigValue::from(options.with_scheduling))?;
    scope.set_config("withDeckConfigs", &ConfigValue::from(options.with_deck_configs))?;
    Ok(())
}

/// The target's today from the live rows, by the pure day calculation.
fn live_today(scope: &WriteScope<'_>, clock: &dyn meguri_model::clock::Clock) -> Result<u32, EngineError> {
    let config = meguri_sched::model::sched_config::SchedConfig::new(scope);
    let at = scope.now().0;
    Ok(meguri_sched::timing::sched_day(&meguri_sched::timing::DayInputs {
        crt: scope.col()?.created.0,
        creation_offset: config.creation_offset()?,
        rollover_hour: config.rollover_hour()?,
        offset: clock.utc_offset_at(at),
        now: at.div_euclid(1000),
    })
    .today)
}

fn note_row(note: &mut Note, record: &Notetype, values: &[String], normalize: bool) {
    let fields: Vec<String> = values
        .iter()
        .map(|value| field_as_saved(value, normalize).into_owned())
        .collect();
    note.sort_field = meguri_text::checksum::sort_text_of(&fields, record.config.sort_field_idx as usize).into_owned();
    note.checksum = meguri_text::checksum::note_checksum(fields.first().map_or("", String::as_str));
    note.fields = fields.join("\u{1f}");
}

/// S5 on the live notes of a note type: every one written and stamped.
fn rewrite_notes(
    scope: &mut WriteScope<'_>,
    notetype: i64,
    record: &Notetype,
    origins: &[Option<usize>],
    normalize: bool,
) -> Result<(), EngineError> {
    let now = scope.now().whole_secs();
    let mut after = None;
    loop {
        let page = scope.notetype_notes_after(NotetypeId(notetype), after, 500)?;
        let Some(last) = page.last() else { break };
        after = Some(last.id);
        for mut note in page {
            let stored: Vec<&str> = note.fields.split('\u{1f}').collect();
            let values: Vec<String> = origins
                .iter()
                .map(|origin| origin.and_then(|at| stored.get(at)).map_or(String::new(), |v| (*v).to_owned()))
                .collect();
            note_row(&mut note, record, &values, normalize);
            note.mtime = now;
            note.usn = Usn::PENDING;
            scope.put_note_forced(&note)?;
        }
    }
    Ok(())
}

/// Change Note Type's effects on one note (M9 steps 2 and 3) for a merge's
/// move: maps that name every old field and template, so nothing is lost
/// but values past the old field count and cards without a template whose
/// ordinal a renumbered card takes.
fn move_note(
    scope: &mut WriteScope<'_>,
    id: NoteId,
    record: &Notetype,
    fields: &[Option<u32>],
    templates: Option<&[Option<u32>]>,
) -> Result<bool, EngineError> {
    let Some(mut note) = scope.note(id)? else {
        return Ok(false);
    };
    let now = scope.now().whole_secs();
    if let Some(templates) = templates {
        let cards = scope.cards_of_note(id)?;
        let target_of = |ord: u16| templates.iter().position(|old| *old == Some(u32::from(ord)));
        for card in &cards {
            if target_of(card.ord).is_none() && templates.get(usize::from(card.ord)).is_some_and(Option::is_some) {
                scope.remove_card(card.id)?;
                scope.add_grave(card.id.0, GraveKind::Card, Usn::PENDING)?;
                scope.retire_card_id(card.id);
            }
        }
        for card in &cards {
            if let Some(to) = target_of(card.ord)
                && to != usize::from(card.ord)
            {
                let mut moved = card.clone();
                moved.ord = u16::try_from(to).unwrap_or(u16::MAX);
                moved.mtime = now;
                moved.usn = Usn::PENDING;
                scope.put(&moved)?;
            }
        }
    }
    let stored: Vec<&str> = note.fields.split('\u{1f}').collect();
    let values: Vec<String> = fields
        .iter()
        .map(|origin| {
            origin
                .and_then(|at| stored.get(at as usize))
                .map_or(String::new(), |v| (*v).to_owned())
        })
        .collect();
    // M9 step 3: control characters removed, no NFC.
    note_row(&mut note, record, &values, false);
    note.notetype_id = record.id;
    note.mtime = now;
    note.usn = Usn::PENDING;
    scope.put_note_forced(&note)?;
    meguri_engine::cardgen::generate_in(scope, record, &note, None)?;
    Ok(true)
}

fn native_human(name: &str) -> String {
    name.replace('\u{1f}', "::")
}

/// I10.2 step 1: the first recorded rename whose old name leads `name`.
fn apply_renames(name: &mut String, renames: &[(String, String)]) {
    for (old, new) in renames {
        let prefix = format!("{old}\u{1f}");
        if name.starts_with(&prefix) {
            *name = format!("{new}\u{1f}{}", &name[prefix.len()..]);
            return;
        }
    }
}

fn with_suffix(name: &str, suffix: &str) -> String {
    format!("{name}{suffix}")
}

/// I10: presets, then decks, against the live rows. Returns the map from
/// package deck ids to target deck ids.
fn write_decks(
    scope: &mut WriteScope<'_>,
    plan: &ImportPlan,
    fault: &Fault,
    counts: &mut ImportCounts,
) -> Result<HashMap<i64, i64>, EngineError> {
    for preset in &plan.taken.presets {
        if scope.deck_config(preset.id)?.is_none() {
            scope.put(preset)?;
            counts.presets_added += 1;
        }
    }
    let now = scope.now().whole_secs();
    let suffix = format!(" {}", now.0);
    let (today, theirs) = (plan.target_today, plan.package_today);
    let mut rows = scope.deck_rows()?;
    let mut renames: Vec<(String, String)> = Vec::new();
    let mut map = HashMap::new();
    for taken in &plan.taken.decks {
        let mut deck = taken.clone();
        deck.name = normalize_native_name(&deck.name);
        apply_renames(&mut deck.name, &renames);
        if let DeckKind::Normal(normal) = &mut deck.kind.kind {
            for limit in [&mut normal.new_limit_today, &mut normal.review_limit_today] {
                if let Some(day) = limit {
                    if day.today == theirs {
                        day.today = today;
                    } else if today > 0 {
                        day.today = day.today.min(today - 1);
                    } else {
                        *limit = None;
                    }
                }
            }
        }
        let filtered = decks::is_filtered(&deck);
        if let Some(found) = names::find(&rows, &deck.name) {
            if decks::is_filtered(found) == filtered {
                // Step 4.
                let mut updated = found.clone();
                match (&mut updated.kind.kind, &deck.kind.kind) {
                    (DeckKind::Normal(stored), DeckKind::Normal(incoming)) => {
                        if !incoming.description.is_empty() {
                            stored.description.clone_from(&incoming.description);
                            stored.markdown_description = incoming.markdown_description;
                        }
                        if incoming.config_id != 1 {
                            stored.config_id = incoming.config_id;
                        }
                        if incoming.review_limit.is_some() {
                            stored.review_limit = incoming.review_limit;
                        }
                        if incoming.new_limit.is_some() {
                            stored.new_limit = incoming.new_limit;
                        }
                        if incoming.review_limit_today.is_some() {
                            stored.review_limit_today.clone_from(&incoming.review_limit_today);
                        }
                        if incoming.new_limit_today.is_some() {
                            stored.new_limit_today.clone_from(&incoming.new_limit_today);
                        }
                    }
                    _ => updated.kind = deck.kind.clone(),
                }
                map.insert(taken.id.0, found.id.0);
                if updated.to_cells() != found.to_cells() {
                    updated.mtime = now;
                    scope.put(&updated)?;
                    counts.decks_updated += 1;
                    let id = updated.id;
                    if let Some(row) = rows.iter_mut().find(|row| row.id == id) {
                        *row = scope.deck(id)?.unwrap_or(updated);
                    }
                }
                continue;
            }
            // The other kind holds the name: a suffixed name, then added.
            let old = deck.name.clone();
            deck.name = with_suffix(&deck.name, &suffix);
            renames.push((old, deck.name.clone()));
        }
        // Step 5.
        loop {
            let mut ancestor = None;
            let mut parent = names::parent(&deck.name);
            while let Some(name) = parent {
                if let Some(found) = names::find(&rows, name) {
                    ancestor = Some(found.clone());
                    break;
                }
                parent = names::parent(name);
            }
            let Some(ancestor) = ancestor.filter(decks::is_filtered) else {
                break;
            };
            let mut stand_in = decks::new_deck(with_suffix(&ancestor.name, &suffix));
            stand_in.name = names::unique(&rows, DeckId(0), stand_in.name);
            stand_in.mtime = now;
            let id = scope.add_deck(&stand_in)?;
            counts.decks_added += 1;
            renames.push((ancestor.name.clone(), stand_in.name.clone()));
            rows.push(scope.deck(id)?.ok_or_else(|| stale(Table::Decks, id.0))?);
            let before = deck.name.clone();
            apply_renames(&mut deck.name, &renames[renames.len() - 1..]);
            if deck.name == before {
                // The ancestor matched under unicase but not byte for byte.
                let rest = &before[ancestor.name.len()..];
                deck.name = format!("{}{rest}", stand_in.name);
            }
        }
        let name = match names::prepare(&rows, deck.name.clone()) {
            Ok(name) => name,
            Err(EngineError::DeckRefused(decks::Refusal::TooDeep)) => {
                return Err(refuse(
                    fault,
                    ApkgImportError::DeckTooDeep {
                        name: native_human(&deck.name),
                    },
                ));
            }
            Err(other) => return Err(other),
        };
        // D4: missing ancestors, the nearest first.
        let mut parent = names::parent(&name);
        while let Some(missing) = parent {
            if names::find(&rows, missing).is_some() {
                break;
            }
            let mut created = decks::new_deck(missing.to_owned());
            created.mtime = now;
            let id = scope.add_deck(&created)?;
            counts.decks_added += 1;
            rows.push(scope.deck(id)?.ok_or_else(|| stale(Table::Decks, id.0))?);
            parent = names::parent(missing);
        }
        deck.name = names::unique(&rows, DeckId(0), name);
        deck.mtime = now;
        deck.usn = Usn::PENDING;
        let id = scope.add_deck(&deck)?;
        counts.decks_added += 1;
        rows.push(scope.deck(id)?.ok_or_else(|| stale(Table::Decks, id.0))?);
        map.insert(taken.id.0, id.0);
    }
    Ok(map)
}

/// The id a new row takes: its package id, or + 999 until `table` has none.
fn free_id(scope: &WriteScope<'_>, table: Table, mut id: i64) -> Result<i64, EngineError> {
    while scope.row_image(table, &RowKey::Id(id))?.is_some() {
        id = id.checked_add(999).ok_or_else(|| EngineError::InvalidArgument {
            message: "no free id".into(),
        })?;
    }
    Ok(id)
}

pub(crate) fn commit(
    ctx: &mut OpContext<'_, '_>,
    mut plan: ImportPlan,
    clock: &dyn meguri_model::clock::Clock,
    fault: &Fault,
) -> Result<Committed, EngineError> {
    let mut counts = ImportCounts {
        notetypes_added: plan.stage.added,
        notetypes_updated: plan.stage.updated,
        notetypes_copied: plan.stage.copied,
        media_written: plan.media.written,
        media_reused: plan.media.reused,
        media_skipped: plan.media.skipped,
        ..ImportCounts::default()
    };
    let mut log = ImportLog {
        found_notes: plan.raw.notes.len() as u64,
        ..ImportLog::default()
    };
    let scope = ctx.scope();
    if live_today(scope, clock)? != plan.target_today {
        return Err(stale(Table::Col, "day"));
    }
    write_options(scope, &plan.options)?;
    let normalize = meguri_engine::notes::fields::normalize_enabled(scope)?;
    let now = scope.now().whole_secs();

    // Note types (I7 or the merge), inside one structure report.
    let reasons: Vec<SchemaReason> = plan.stage.reasons.clone();
    meguri_engine::mark_schema_change(scope, &reasons)?;
    let mut report = plan.stage.report.clone();
    report.reasons.insert(StructureReason::ApkgApply);
    let token = scope.start_structure_report(report)?;
    let mut copies: Vec<i64> = Vec::new();
    for write in &plan.stage.writes {
        match write {
            TypeWrite::Put { record, stamp_now } => {
                let mut record = record.clone();
                if *stamp_now {
                    record.mtime = now;
                }
                record.usn = Usn::PENDING;
                scope.put_notetype(&record)?;
            }
            TypeWrite::AddCopy { record, stamp_now } => {
                let mut record = record.clone();
                if *stamp_now {
                    record.mtime = now;
                }
                record.usn = Usn::PENDING;
                let id = scope.add_notetype(&record)?;
                scope.set_config("curModel", &ConfigValue::from(id.0))?;
                copies.push(id.0);
            }
            TypeWrite::RewriteNotes {
                notetype,
                record,
                origins,
            } => rewrite_notes(scope, *notetype, record, origins, normalize)?,
            TypeWrite::Regenerate(planned) => {
                regenerate::commit_in(scope, planned)?;
            }
            TypeWrite::MoveNotes {
                from,
                record,
                fields,
                templates,
                notes,
            } => {
                let ids: Vec<NoteId> = match notes {
                    Some(ids) => ids.clone(),
                    None => {
                        let mut ids = Vec::new();
                        let mut after = None;
                        loop {
                            let page = scope.notetype_notes_after(NotetypeId(*from), after, 500)?;
                            let Some(last) = page.last() else { break };
                            after = Some(last.id);
                            ids.extend(page.iter().map(|note| note.id));
                        }
                        ids
                    }
                };
                for id in ids {
                    if move_note(scope, id, record, fields, templates.as_deref())? {
                        counts.notes_moved += 1;
                    }
                }
            }
        }
    }
    let resolve = |target: TypeTarget| -> Result<i64, EngineError> {
        match target {
            TypeTarget::Id(id) => Ok(id),
            TypeTarget::Copy(index) => copies.get(index).copied().ok_or_else(|| EngineError::InvalidArgument {
                message: "an import copy was not added".into(),
            }),
        }
    };

    // Notes (I9).
    let mut targets: Vec<Option<NoteId>> = vec![None; plan.notes.len()];
    let mut written: HashSet<usize> = HashSet::new();
    for index in 0..plan.notes.len() {
        let planned = &plan.notes[index];
        let source = &plan.raw.notes[index];
        match planned.decision {
            Decision::MissingNotetype => log.missing_notetype.push(entry(source.id.0, &planned.fields, None)),
            Decision::Conflicting { existing } => {
                log.conflicting.push(entry(source.id.0, &planned.fields, Some(existing.0)));
            }
            Decision::Duplicate { target } => {
                log.duplicate.push(entry(target.0, &planned.fields, None));
                targets[index] = Some(target);
            }
            Decision::DuplicateOf { earlier, same_type } => {
                let target = targets[earlier].ok_or_else(|| stale(Table::Notes, source.id.0))?;
                log.duplicate.push(entry(target.0, &planned.fields, None));
                if same_type {
                    targets[index] = Some(target);
                }
            }
            Decision::New | Decision::Updated { .. } => {
                let route = plan
                    .stage
                    .routes
                    .get(&planned.package_type)
                    .ok_or_else(|| stale(Table::Notetypes, planned.package_type))?;
                let mid = resolve(route.target)?;
                let tags = meguri_engine::tags::note_tags_for_save(scope, &[plan.taken.tags[index].as_str()])?;
                let mut note = match planned.decision {
                    Decision::Updated { target } => scope.note(target)?.ok_or_else(|| stale(Table::Notes, target.0))?,
                    _ => Note {
                        id: NoteId(free_id(scope, Table::Notes, source.id.0)?),
                        guid: source.guid.clone(),
                        flags: 0,
                        data: String::new(),
                        ..Note::default()
                    },
                };
                if matches!(planned.decision, Decision::Updated { .. }) && note.notetype_id.0 != mid {
                    return Err(stale(Table::Notes, note.id.0));
                }
                note_row(&mut note, &route.record, &planned.fields, normalize);
                note.notetype_id = NotetypeId(mid);
                note.tags = tags;
                note.mtime = EpochSecs(source.mtime.0);
                note.usn = Usn::PENDING;
                scope.put(&note)?;
                let stored: Vec<String> = note.fields.split('\u{1f}').map(str::to_owned).collect();
                if matches!(planned.decision, Decision::New) {
                    log.new.push(entry(note.id.0, &stored, None));
                    counts.notes_added += 1;
                } else {
                    log.updated.push(entry(note.id.0, &stored, None));
                    counts.notes_updated += 1;
                }
                targets[index] = Some(note.id);
                written.insert(index);
            }
        }
    }

    // J8: merged note types no note uses any more.
    let mut removed = false;
    for id in &plan.stage.removals {
        if scope.notetype_note_count(NotetypeId(*id))? != 0 {
            return Err(stale(Table::Notetypes, id));
        }
        scope.remove_config_family(&format!("_nt_{id}_"))?;
        scope.remove_notetype(NotetypeId(*id))?;
        counts.notetypes_removed += 1;
        removed = true;
    }
    scope.complete_structure_report(token)?;
    if removed && let Some(first) = scope.first_notetype_id_by_name()? {
        scope.set_config("curModel", &ConfigValue::from(first.0))?;
    }

    let decks = write_decks(scope, &plan, fault, &mut counts)?;

    // Cards (I11.1) and review log (I11.3).
    let index_of: HashMap<i64, usize> = plan.raw.notes.iter().enumerate().map(|(i, note)| (note.id.0, i)).collect();
    let shift = i64::from(plan.package_today) - i64::from(plan.target_today);
    let mut pairs: HashMap<NoteId, HashSet<u16>> = HashMap::new();
    let mut added_cards: HashMap<i64, i64> = HashMap::new();
    let mut missing_deck: HashSet<usize> = HashSet::new();
    for card in &plan.taken.cards {
        let Some(&index) = index_of.get(&card.note_id.0) else {
            continue;
        };
        let Some(note) = targets[index] else { continue };
        let route = plan
            .stage
            .routes
            .get(&plan.notes[index].package_type)
            .ok_or_else(|| stale(Table::Notetypes, card.note_id.0))?;
        let ord = match &route.template_map {
            None => card.ord,
            Some(map) => match map.get(usize::from(card.ord)) {
                Some(to) => *to,
                // An ordinal without an entry keeps its value only when
                // the final list has no template there.
                None if usize::from(card.ord) >= route.record.templates.len() => card.ord,
                None => continue,
            },
        };
        let seen = match pairs.entry(note) {
            std::collections::hash_map::Entry::Occupied(entry) => entry.into_mut(),
            std::collections::hash_map::Entry::Vacant(entry) => {
                let existing = if written.contains(&index) && matches!(plan.notes[index].decision, Decision::New) {
                    HashSet::new()
                } else {
                    scope.cards_of_note(note)?.iter().map(|card| card.ord).collect()
                };
                entry.insert(existing)
            }
        };
        if !seen.insert(ord) {
            continue;
        }
        let mut place = |id: DeckId, home: bool| -> DeckId {
            if home && id.0 == 0 {
                return id;
            }
            match decks.get(&id.0) {
                Some(mapped) => DeckId(*mapped),
                None if id.0 == 1 => id,
                None => {
                    missing_deck.insert(index);
                    DeckId(1)
                }
            }
        };
        let mut new = card.clone();
        new.id = CardId(free_id(scope, Table::Cards, card.id.0)?);
        new.note_id = note;
        new.ord = ord;
        new.deck_id = place(card.deck_id, false);
        new.original_deck_id = place(card.original_deck_id, true);
        if plan.options.with_scheduling {
            let review = card.card_type == meguri_model::card::CardType::Review;
            if matches!(card.queue.stored(), 2 | 3) || review {
                new.due = i32::try_from(i64::from(card.due) - shift).unwrap_or(card.due);
            }
            if review && card.original_due != 0 {
                new.original_due = i32::try_from(i64::from(card.original_due) - shift).unwrap_or(card.original_due);
            }
        }
        new.usn = Usn::PENDING;
        scope.put(&new)?;
        added_cards.insert(card.id.0, new.id.0);
        counts.cards_added += 1;
    }
    for (index, planned) in plan.notes.iter().enumerate() {
        if missing_deck.contains(&index)
            && let Some(target) = targets[index]
        {
            let stored: Vec<String> = match scope.note(target)? {
                Some(note) if written.contains(&index) => note.fields.split('\u{1f}').map(str::to_owned).collect(),
                _ => planned.fields.clone(),
            };
            log.missing_deck.push(entry(target.0, &stored, None));
        }
    }
    for row in &plan.taken.revlog {
        let Some(card) = added_cards.get(&row.card_id.0) else {
            continue;
        };
        let mut row = row.clone();
        row.card_id = CardId(*card);
        row.usn = Usn::PENDING;
        if scope.insert_revlog(&row)?.changed() {
            counts.revlog_added += 1;
        }
    }

    // The staged media travel with the commit (M13); their witnesses are
    // the notes whose fields name them.
    if let Some(stage) = plan.media.stage.take() {
        let witnesses: Vec<NoteId> = plan.media.witnesses.iter().filter_map(|index| targets[*index]).collect();
        ctx.commit_media(stage, &witnesses)?;
    }
    Ok(Committed {
        log,
        counts,
        statics: std::mem::take(&mut plan.media.statics),
    })
}
