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
use meguri_model::deck::{Deck, normalize_native_name};
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
use super::gather::PackageRows;
use super::plan::{Decision, ImportPlan, routed_fields};
use super::{ApkgImportError, ImportCounts, ImportLog, ImportOptions, LogEntry};

/// What the write phase hands back.
#[derive(Clone, Debug, Default)]
pub(crate) struct Committed {
    pub log: ImportLog,
    pub counts: ImportCounts,
    /// Static media entries to write once the op committed (I12.4).
    /// The structure report the op committed with (`engine/change-stream`
    /// S2.5), which the storage guard checked against the rows.
    pub structure: meguri_storage::journal::StructureReport,
    pub statics: Vec<usize>,
    /// Decoded bytes charged to the package so far (P8).
    pub decoded: u64,
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

/// What the log's field texts may hold in all (R1852): past it, an entry
/// keeps its ids and no fields, and `ImportLog::fields_omitted` counts it.
/// A note type of many fields would otherwise make the log grow with the
/// package's text.
pub(crate) const LOG_TEXT_BUDGET: usize = 16 * 1024 * 1024;

struct LogBudget {
    left: usize,
    omitted: u64,
}

impl LogBudget {
    fn entry(&mut self, note: i64, fields: &[String], existing: Option<i64>) -> LogEntry {
        let mut kept = Vec::new();
        if self.left > 0 {
            kept = fields.iter().map(|field| reduced(field)).collect();
            let cost: usize =
                kept.iter().map(|field: &String| field.len() + std::mem::size_of::<String>()).sum();
            self.left = self.left.saturating_sub(cost.max(1));
        } else {
            self.omitted += 1;
        }
        LogEntry {
            note,
            fields: kept,
            existing_id: existing,
        }
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

/// The target's today at `at` (epoch milliseconds) from `rows`, by the pure
/// day calculation: the plan uses it on its snapshot and the write phase on
/// the live rows.
pub(crate) fn today(
    rows: &impl ReadRows,
    clock: &dyn meguri_model::clock::Clock,
    at: i64,
) -> Result<u32, EngineError> {
    let config = meguri_sched::model::sched_config::SchedConfig::new(rows);
    Ok(meguri_sched::timing::sched_day(&meguri_sched::timing::DayInputs {
        crt: rows.col()?.created.0,
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
    retired: &mut HashSet<i64>,
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
                retired.insert(card.id.0);
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

/// Where I10.2 writes: the live scope in the write phase, nothing in the
/// plan's dry run (which only looks for `DeckTooDeep`).
pub(crate) trait DeckSink {
    /// Writes a changed deck and returns it as stored.
    fn put(&mut self, deck: &Deck) -> Result<Deck, EngineError>;
    /// Adds a deck under a new id and returns it as stored.
    fn add(&mut self, deck: &Deck) -> Result<Deck, EngineError>;
}

struct LiveDecks<'s, 'c> {
    scope: &'s mut WriteScope<'c>,
}

impl DeckSink for LiveDecks<'_, '_> {
    fn put(&mut self, deck: &Deck) -> Result<Deck, EngineError> {
        self.scope.put(deck)?;
        Ok(self.scope.deck(deck.id)?.unwrap_or_else(|| deck.clone()))
    }

    fn add(&mut self, deck: &Deck) -> Result<Deck, EngineError> {
        let id = self.scope.add_deck(deck)?;
        self.scope.deck(id)?.ok_or_else(|| stale(Table::Decks, id.0))
    }
}

/// The plan's sink: decks get placeholder ids no row has.
pub(crate) struct DryDecks {
    next: i64,
}

impl DryDecks {
    pub fn new() -> DryDecks {
        DryDecks { next: -1 }
    }
}

impl DeckSink for DryDecks {
    fn put(&mut self, deck: &Deck) -> Result<Deck, EngineError> {
        Ok(deck.clone())
    }

    fn add(&mut self, deck: &Deck) -> Result<Deck, EngineError> {
        let mut added = deck.clone();
        added.id = DeckId(self.next);
        self.next -= 1;
        Ok(added)
    }
}

/// Why I10.2 stopped.
pub(crate) enum DeckFault {
    /// D3's nesting limit: the human name.
    TooDeep(String),
    Engine(EngineError),
}

impl From<EngineError> for DeckFault {
    fn from(error: EngineError) -> Self {
        DeckFault::Engine(error)
    }
}

/// What I10.2 did, for the result's counts.
#[derive(Default)]
pub(crate) struct DeckCounts {
    pub added: u64,
    pub updated: u64,
}

/// I10.2 over `rows`, the target's decks as the caller reads them (the
/// plan's snapshot or the live rows), writing through `sink`. Returns the
/// map from package deck ids to target deck ids.
pub(crate) fn place_decks(
    mut rows: Vec<Deck>,
    taken: &[Deck],
    today: u32,
    theirs: u32,
    now: EpochSecs,
    sink: &mut dyn DeckSink,
    counts: &mut DeckCounts,
) -> Result<HashMap<i64, i64>, DeckFault> {
    let suffix = format!(" {}", now.0);
    let mut renames: Vec<(String, String)> = Vec::new();
    let mut map = HashMap::new();
    for taken in taken {
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
                    let stored = sink.put(&updated)?;
                    counts.updated += 1;
                    let id = updated.id;
                    if let Some(row) = rows.iter_mut().find(|row| row.id == id) {
                        *row = stored;
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
            let stored = sink.add(&stand_in)?;
            counts.added += 1;
            renames.push((ancestor.name.clone(), stand_in.name.clone()));
            rows.push(stored);
            let before = deck.name.clone();
            apply_renames(&mut deck.name, &renames[renames.len() - 1..]);
            if deck.name == before {
                // The ancestor matched under unicase but not byte for byte:
                // replace as many leading components as its name has.
                let depth = ancestor.name.split('\u{1f}').count();
                let rest: Vec<&str> = before.split('\u{1f}').skip(depth).collect();
                deck.name = std::iter::once(stand_in.name.as_str())
                    .chain(rest)
                    .collect::<Vec<_>>()
                    .join("\u{1f}");
            }
        }
        let name = match names::prepare(&rows, deck.name.clone()) {
            Ok(name) => name,
            Err(EngineError::DeckRefused(decks::Refusal::TooDeep)) => {
                return Err(DeckFault::TooDeep(native_human(&deck.name)));
            }
            Err(other) => return Err(other.into()),
        };
        // D4: missing ancestors, the nearest first.
        let mut parent = names::parent(&name);
        while let Some(missing) = parent {
            if names::find(&rows, missing).is_some() {
                break;
            }
            let mut created = decks::new_deck(missing.to_owned());
            created.mtime = now;
            let stored = sink.add(&created)?;
            counts.added += 1;
            rows.push(stored);
            parent = names::parent(missing);
        }
        deck.name = names::unique(&rows, DeckId(0), name);
        deck.mtime = now;
        deck.usn = Usn::PENDING;
        let stored = sink.add(&deck)?;
        counts.added += 1;
        map.insert(taken.id.0, stored.id.0);
        rows.push(stored);
    }
    Ok(map)
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
            // The golden `package/apkg-import/v1-scheduler` shows the added
            // row holding the bounded values of a preset save
            // (`sched/presets` V3), not the bytes the package stores.
            scope.put(&preset.for_use())?;
            counts.presets_added += 1;
        }
    }
    let now = scope.now().whole_secs();
    let rows = scope.deck_rows()?;
    let mut placed = DeckCounts::default();
    let outcome = place_decks(
        rows,
        &plan.taken.decks,
        plan.target_today,
        plan.package_today,
        now,
        &mut LiveDecks { scope },
        &mut placed,
    );
    counts.decks_added += placed.added;
    counts.decks_updated += placed.updated;
    match outcome {
        Ok(map) => Ok(map),
        Err(DeckFault::TooDeep(name)) => Err(refuse(fault, ApkgImportError::DeckTooDeep { name })),
        Err(DeckFault::Engine(error)) => Err(error),
    }
}

/// The id a new row takes: its package id, or + 999 until `table` has none
/// and this op retired none there (a card removed with its grave keeps its
/// id to itself).
fn free_id(scope: &WriteScope<'_>, table: Table, mut id: i64, retired: &HashSet<i64>) -> Result<i64, EngineError> {
    while retired.contains(&id) || scope.row_image(table, &RowKey::Id(id))?.is_some() {
        id = id.checked_add(999).ok_or_else(|| EngineError::InvalidArgument {
            message: "no free id".into(),
        })?;
    }
    Ok(id)
}

pub(crate) fn commit(
    ctx: &mut OpContext<'_, '_>,
    mut plan: ImportPlan,
    package_path: &std::path::Path,
    context: &super::ApkgContext,
    fault: &Fault,
) -> Result<Committed, EngineError> {
    let clock = context.clock.as_ref();
    let mut counts = ImportCounts {
        notetypes_added: plan.stage.added,
        notetypes_updated: plan.stage.updated,
        notetypes_copied: plan.stage.copied,
        media_written: plan.media.written,
        media_reused: plan.media.reused,
        media_skipped: plan.media.skipped,
        ..ImportCounts::default()
    };
    let mut budget = LogBudget {
        left: LOG_TEXT_BUDGET,
        omitted: 0,
    };
    let mut log = ImportLog {
        found_notes: plan.raw.notes.len() as u64,
        ..ImportLog::default()
    };
    let scope = ctx.scope();
    scope.stamp_keyed_config_now();
    let at = scope.now().0;
    if today(&*scope, clock, at)? != plan.target_today {
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
    let authored = report.clone();
    let token = scope.start_structure_report(report)?;
    let mut copies: Vec<i64> = Vec::new();
    let mut retired: HashSet<i64> = HashSet::new();
    let unused: HashSet<i64> = HashSet::new();
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
                let mut id = scope.add_notetype(&record)?;
                if plan.stage.routes.contains_key(&id.0) {
                    // The drawn id is one a package note type is added
                    // under later in this pass: the copy takes the next id
                    // that no row and no such note type holds.
                    scope.remove_notetype(id)?;
                    while plan.stage.routes.contains_key(&id.0) || scope.notetype(id)?.is_some() {
                        id = NotetypeId(id.0 + 1);
                    }
                    record.id = id;
                    scope.put_notetype(&record)?;
                }
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
                    if move_note(scope, id, record, fields, templates.as_deref(), &mut retired)? {
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

    // Notes (I9), read a page at a time from the staged package collection.
    let package = PackageRows::open(package_path, context).map_err(|error| refuse(fault, error))?;
    let mut targets: Vec<Option<NoteId>> = vec![None; plan.notes.len()];
    let mut written: HashSet<usize> = HashSet::new();
    let mut witnesses: Vec<NoteId> = Vec::new();
    let mut index = 0_usize;
    let mut after = None;
    loop {
        let page = package.notes_after(after).map_err(|error| refuse(fault, error))?;
        let Some(last) = page.last() else { break };
        after = Some(last.id.0);
        for source in page {
            let (Some(planned), Some(head)) = (plan.notes.get(index), plan.raw.notes.get(index)) else {
                return Err(stale(Table::Notes, source.id.0));
            };
            if head.id != source.id.0 {
                return Err(stale(Table::Notes, source.id.0));
            }
            let route = plan.stage.routes.get(&planned.package_type);
            let mut fields = match route {
                Some(route) => routed_fields(&source.fields, route.field_map.as_ref(), &route.record),
                None => source.fields.split('\u{1f}').map(str::to_owned).collect(),
            };
            match planned.decision {
                Decision::MissingNotetype => log.missing_notetype.push(budget.entry(source.id.0, &fields, None)),
                Decision::Conflicting { existing } => {
                    log.conflicting.push(budget.entry(source.id.0, &fields, Some(existing.0)));
                }
                Decision::Duplicate { target } => {
                    log.duplicate.push(budget.entry(target.0, &fields, None));
                    targets[index] = Some(target);
                }
                Decision::DuplicateOf { earlier, same_type } => {
                    let target = targets[earlier].ok_or_else(|| stale(Table::Notes, source.id.0))?;
                    log.duplicate.push(budget.entry(target.0, &fields, None));
                    if same_type {
                        targets[index] = Some(target);
                    }
                }
                Decision::New | Decision::Updated { .. } => {
                    let route = route.ok_or_else(|| stale(Table::Notetypes, planned.package_type))?;
                    let mid = resolve(route.target)?;
                    // I12.2, with the stored names staging decided.
                    let witness = super::media::rewrite_note(&plan.media, &mut fields);
                    let tags = meguri_engine::tags::note_tags_for_save(scope, &[plan.taken.tags(&source.tags).as_str()])?;
                    let mut note = match planned.decision {
                        Decision::Updated { target } => {
                            scope.note(target)?.ok_or_else(|| stale(Table::Notes, target.0))?
                        }
                        _ => Note {
                            id: NoteId(free_id(scope, Table::Notes, source.id.0, &unused)?),
                            guid: source.guid.clone(),
                            flags: 0,
                            data: String::new(),
                            ..Note::default()
                        },
                    };
                    if matches!(planned.decision, Decision::Updated { .. }) && note.notetype_id.0 != mid {
                        return Err(stale(Table::Notes, note.id.0));
                    }
                    note_row(&mut note, &route.record, &fields, normalize);
                    note.notetype_id = NotetypeId(mid);
                    note.tags = tags;
                    note.mtime = EpochSecs(source.mtime.0);
                    note.usn = Usn::PENDING;
                    scope.put(&note)?;
                    let stored: Vec<String> = note.fields.split('\u{1f}').map(str::to_owned).collect();
                    if matches!(planned.decision, Decision::New) {
                        log.new.push(budget.entry(note.id.0, &stored, None));
                        counts.notes_added += 1;
                    } else {
                        log.updated.push(budget.entry(note.id.0, &stored, None));
                        counts.notes_updated += 1;
                    }
                    if witness {
                        witnesses.push(note.id);
                    }
                    targets[index] = Some(note.id);
                    written.insert(index);
                }
            }
            index += 1;
        }
    }
    if index != plan.notes.len() {
        return Err(stale(Table::Notes, "package"));
    }
    meguri_model::faultpoint!("package.apkg.write_phase").map_err(meguri_storage::StorageError::from)?;

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

    // Cards (I11.1) and review log (I11.3), a page at a time.
    let index_of: HashMap<i64, usize> = plan.raw.notes.iter().enumerate().map(|(i, note)| (note.id, i)).collect();
    let shift = i64::from(plan.package_today) - i64::from(plan.target_today);
    // The (note, ordinal) pairs seen: one flat set, and the target notes
    // whose existing cards were read into it.
    let mut pairs: HashSet<(i64, u16)> = HashSet::new();
    let mut loaded: HashSet<i64> = HashSet::new();
    let mut added_cards: HashMap<i64, i64> = HashMap::new();
    let mut missing_deck: std::collections::BTreeSet<usize> = std::collections::BTreeSet::new();
    let mut shaper = plan.taken.shaper();
    let mut after = None;
    loop {
        let page = package.cards_after(after).map_err(|error| refuse(fault, error))?;
        let Some(last) = page.last() else { break };
        after = Some(last.id.0);
        for mut card in page {
            // I6: only the cards of the package's notes are taken.
            let Some(&index) = index_of.get(&card.note_id.0) else {
                continue;
            };
            shaper.shape(&mut card);
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
            // A note this import added has no card but those added here.
            let fresh = written.contains(&index) && matches!(plan.notes[index].decision, Decision::New);
            if !fresh && loaded.insert(note.0) {
                for existing in scope.cards_of_note(note)? {
                    pairs.insert((note.0, existing.ord));
                }
            }
            if !pairs.insert((note.0, ord)) {
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
            new.id = CardId(free_id(scope, Table::Cards, card.id.0, &retired)?);
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
    }
    // `missing_deck` entries, in I9's order: the note as written, or as the
    // package holds it when this import wrote nothing for it.
    let missing_deck: Vec<usize> = missing_deck.into_iter().collect();
    for chunk in missing_deck.chunks(200) {
        let ids: Vec<NoteId> = chunk.iter().map(|index| NoteId(plan.raw.notes[*index].id)).collect();
        let held = package.rows().notes_with_ids(&ids)?;
        for index in chunk {
            let Some(target) = targets[*index] else { continue };
            let stored: Vec<String> = match scope.note(target)? {
                Some(note) if written.contains(index) => note.fields.split('\u{1f}').map(str::to_owned).collect(),
                _ => {
                    let source = held.iter().find(|note| note.id.0 == plan.raw.notes[*index].id);
                    let route = plan.stage.routes.get(&plan.notes[*index].package_type);
                    match (source, route) {
                        (Some(source), Some(route)) => {
                            routed_fields(&source.fields, route.field_map.as_ref(), &route.record)
                        }
                        _ => Vec::new(),
                    }
                }
            };
            log.missing_deck.push(budget.entry(target.0, &stored, None));
        }
    }
    if plan.options.with_scheduling {
        let mut after = None;
        loop {
            let page = package.revlog_after(after).map_err(|error| refuse(fault, error))?;
            let Some(last) = page.last() else { break };
            after = Some(last.id.0);
            for mut row in page {
                let Some(card) = added_cards.get(&row.card_id.0) else {
                    continue;
                };
                row.card_id = CardId(*card);
                row.usn = Usn::PENDING;
                if scope.insert_revlog(&row)?.changed() {
                    counts.revlog_added += 1;
                }
            }
        }
    }
    drop(package);

    // The staged media travel with the commit (M13); their witnesses are
    // the notes whose fields name them.
    if let Some(stage) = plan.media.stage.take() {
        ctx.commit_media(stage, &witnesses)?;
    }
    log.fields_omitted = budget.omitted;
    Ok(Committed {
        log,
        counts,
        structure: authored,
        statics: std::mem::take(&mut plan.media.statics),
        decoded: plan.media.decoded,
    })
}
