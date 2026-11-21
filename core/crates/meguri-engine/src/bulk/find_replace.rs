//! Find and Replace (`specs/browser/find-replace.md` F1 to F11) as a bulk
//! action (`specs/browser/bulk-framework.md`).
//!
//! The refusals that need no notes come first, before any note is read:
//! `InvalidArgument` (an empty or repeated target list, an empty field
//! name), F7 step 1's separator in a Tags replacement, then F4 step 5's
//! pattern that does not compile. The compute phase then reads the notes on
//! a maintenance snapshot, a page at a time, and keeps only the ids of the
//! notes the rewrite changes (fields) or that match (tags), with the
//! refusals of F6 step 7. The write transaction re-reads each of those notes
//! from its live row and applies the same rewrite again, so no note is held
//! in memory beyond its own turn; the notes, note types and their parts must
//! be untouched since the snapshot (`TableUntouched`, `engine/op-model` O7),
//! so what the commit writes is what the compute phase found. Matching and
//! rewriting are pure functions ([`Pattern`], [`rewrite_fields`],
//! [`rewrite_tags`]) that a preview can run without writing (U265).

use std::borrow::Cow;
use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;

use meguri_model::ids::{NoteId, NotetypeId};
use meguri_model::note::Note;
use meguri_model::notetype::Notetype;
use meguri_storage::{ReadRows, ReaderRole, Table, WriteScope};
use meguri_text::fold::{compare_folded, folded};
use regex::{NoExpand, Regex};

use super::{BulkCounts, BulkJob, BulkOutcome, NoteTarget, ResolvedNotes, check_ids, op_spec};
use crate::cancel::CancelToken;
use crate::collection::Collection;
use crate::error::EngineError;
use crate::executor::op::{OpContext, OpResult};
use crate::executor::plan_commit::{Plan, PlanContext, plan_and_commit_with};
use crate::executor::preconditions::Precondition;
use crate::lifecycle::Engine;
use crate::progress::ProgressSink;
use crate::tags::registry::{Spelling, new_row, spelling_of};
use crate::tags::{join_note_tags, normalize, split_text};
use crate::undo::names::{FIND_AND_REPLACE, UPDATE_TAG};

/// The engine op's name (`engine/op-model` O1, F11).
pub const OP_NAME: &str = "find_replace";

/// Notes read per page in the compute phase.
const PAGE: usize = 2_000;

/// Where the find text is looked for (F1's In).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FindIn {
    /// Every stored field value (F6 step 2).
    AllFields,
    /// The notes' tags (F7).
    Tags,
    /// The field of this name, compared under `unicase` (F6 step 2).
    Field(String),
}

/// F11's input besides the target.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FindReplace {
    pub find: String,
    pub replace: String,
    /// Treat input as regular expression (F4 step 2).
    pub regex: bool,
    /// F4 step 3.
    pub ignore_case: bool,
    pub within: FindIn,
}

/// Why Find and Replace refused (F11); nothing was written and both undo
/// histories are as they were (F9).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// F7 step 1 (`InvalidInput`): a Tags replacement holding U+0020 or
    /// U+3000.
    SeparatorInReplacement,
    /// F4 step 5: the pattern does not compile; the `regex` crate's message.
    InvalidRegex { message: String },
    /// F6 step 7: a matched note whose stored field count differs from its
    /// note type's, the first in F6 step 1's order.
    FieldCountMismatch { note: NoteId },
    /// F6 step 7 (`NotFound{noteId}`): a target note whose note type does
    /// not exist, the first in F6 step 1's order.
    NotetypeMissing { note: NoteId },
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Refusal::SeparatorInReplacement => f.write_str("a tag replacement cannot contain a space"),
            Refusal::InvalidRegex { message } => write!(f, "invalid regular expression: {message}"),
            Refusal::FieldCountMismatch { note } => {
                write!(f, "note {} holds a different number of fields than its note type", note.0)
            }
            Refusal::NotetypeMissing { note } => write!(f, "the note type of note {} does not exist", note.0),
        }
    }
}

impl From<Refusal> for EngineError {
    fn from(refusal: Refusal) -> EngineError {
        EngineError::FindReplace(refusal)
    }
}

fn invalid(message: &str) -> EngineError {
    EngineError::InvalidArgument {
        message: message.to_owned(),
    }
}

/// The checks of F11 that need no collection, in its order:
/// `InvalidArgument` for an empty field name, then F7 step 1. A target
/// list is checked by [`check_ids`].
pub fn check_request(request: &FindReplace) -> Result<(), EngineError> {
    if matches!(&request.within, FindIn::Field(name) if name.is_empty()) {
        return Err(invalid("Find and Replace needs a field name (F11)"));
    }
    if request.within == FindIn::Tags && request.replace.chars().any(crate::tags::canonical::is_separator) {
        return Err(Refusal::SeparatorInReplacement.into());
    }
    Ok(())
}

/// F4's pattern and F5's replacement.
#[derive(Clone, Debug)]
pub struct Pattern {
    regex: Regex,
    replace: String,
    expand: bool,
}

impl Pattern {
    /// F4 steps 1 to 5. `normalize_note_text` is the collection's key
    /// (`browser/config-keys` K1); Tags never takes the NFC step.
    pub fn build(request: &FindReplace, normalize_note_text: bool) -> Result<Pattern, Refusal> {
        let mut text = if request.regex {
            request.find.clone()
        } else {
            regex::escape(&request.find)
        };
        if request.ignore_case {
            text.insert_str(0, "(?i)");
        }
        if normalize_note_text && request.within != FindIn::Tags {
            text = meguri_text::normalize::nfc(&text).into_owned();
        }
        let regex = Regex::new(&text).map_err(|error| Refusal::InvalidRegex {
            message: error.to_string(),
        })?;
        Ok(Pattern {
            regex,
            replace: request.replace.clone(),
            expand: request.regex,
        })
    }

    /// Whether the pattern has at least one match in `text` (an empty
    /// pattern matches every text, the empty one included).
    pub fn is_match(&self, text: &str) -> bool {
        self.regex.is_match(text)
    }

    /// F5: every match replaced, left to right, without overlap.
    pub fn replace<'t>(&self, text: &'t str) -> Cow<'t, str> {
        if self.expand {
            self.regex.replace_all(text, self.replace.as_str())
        } else {
            self.regex.replace_all(text, NoExpand(&self.replace))
        }
    }
}

/// What F6 makes of one note.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FieldsRewrite {
    /// The note type has no field of that name: not read (F6 step 2).
    Skipped,
    /// No field it reads matches.
    Unmatched,
    /// It matches, and its rewritten and normalized fields equal the stored
    /// ones: not written, not counted (F6 step 6).
    Unchanged,
    /// The stored values to write.
    Changed(Vec<String>),
}

/// The ordinal of `name` in `notetype` under `unicase` (F6 step 2).
fn field_ordinal(notetype: &Notetype, name: &str) -> Option<usize> {
    notetype
        .fields
        .iter()
        .position(|field| compare_folded(&field.name, name).is_eq())
}

/// F6 steps 2, 3, 4 (fields) and 6, and step 7's field count, for one note
/// of an existing note type.
pub fn rewrite_fields(
    note: &Note,
    notetype: &Notetype,
    within: &FindIn,
    pattern: &Pattern,
    normalize_note_text: bool,
) -> Result<FieldsRewrite, Refusal> {
    let values: Vec<&str> = note.field_values().collect();
    let read: Vec<usize> = match within {
        FindIn::AllFields => (0..values.len()).collect(),
        FindIn::Field(name) => match field_ordinal(notetype, name) {
            None => return Ok(FieldsRewrite::Skipped),
            // A row holding fewer values has nothing there to match.
            Some(ord) => (ord < values.len()).then_some(ord).into_iter().collect(),
        },
        FindIn::Tags => return Ok(FieldsRewrite::Unmatched),
    };
    let matched: Vec<usize> = read
        .into_iter()
        .filter(|&ord| pattern.is_match(values[ord]))
        .collect();
    if matched.is_empty() {
        return Ok(FieldsRewrite::Unmatched);
    }
    if values.len() != notetype.fields.len() {
        return Err(Refusal::FieldCountMismatch { note: note.id });
    }
    let fields: Vec<String> = values
        .iter()
        .enumerate()
        .map(|(ord, value)| {
            let value = if matched.contains(&ord) {
                pattern.replace(value)
            } else {
                Cow::Borrowed(*value)
            };
            meguri_text::normalize::field_as_saved(&value, normalize_note_text).into_owned()
        })
        .collect();
    if joined_equals(&fields, &note.fields) {
        Ok(FieldsRewrite::Unchanged)
    } else {
        Ok(FieldsRewrite::Changed(fields))
    }
}

/// Whether `fields` joined at U+001F equal `stored`, without joining.
fn joined_equals(fields: &[String], stored: &str) -> bool {
    let mut rest = stored;
    for (i, field) in fields.iter().enumerate() {
        if i > 0 {
            match rest.strip_prefix('\u{1f}') {
                Some(after) => rest = after,
                None => return false,
            }
        }
        match rest.strip_prefix(field.as_str()) {
            Some(after) => rest = after,
            None => return false,
        }
    }
    rest.is_empty()
}

/// F7 step 3 for one note's stored tags: `None` when no tag matches (a note
/// without tags never does); otherwise every tag with F5's result in place
/// of the matched ones, in stored order (F7 step 4's input).
pub fn rewrite_tags(stored: &str, pattern: &Pattern) -> Option<Vec<String>> {
    let tags: Vec<&str> = split_text(stored).collect();
    if !tags.iter().any(|tag| pattern.is_match(tag)) {
        return None;
    }
    Some(
        tags.iter()
            .map(|tag| {
                if pattern.is_match(tag) {
                    pattern.replace(tag).into_owned()
                } else {
                    (*tag).to_owned()
                }
            })
            .collect(),
    )
}

/// What the compute phase found.
#[derive(Debug)]
struct Planned {
    pattern: Arc<Pattern>,
    normalize: bool,
    /// Fields: the notes the rewrite changes, in F6 step 1's order. Tags:
    /// the notes that match, ascending.
    notes: Vec<(NotetypeId, NoteId)>,
    considered: u64,
}

/// The first refusal in F6 step 1's order, kept while the notes are read in
/// any order.
#[derive(Default)]
struct FirstRefusal(Option<((NotetypeId, NoteId), Refusal)>);

impl FirstRefusal {
    fn note(&mut self, at: (NotetypeId, NoteId), refusal: Refusal) {
        if self.0.as_ref().is_none_or(|(first, _)| at < *first) {
            self.0 = Some((at, refusal));
        }
    }

    fn check(self) -> Result<(), EngineError> {
        match self.0 {
            Some((_, refusal)) => Err(refusal.into()),
            None => Ok(()),
        }
    }
}

/// Every note type by id, read once.
struct Notetypes<'r, R: ReadRows + ?Sized> {
    reader: &'r R,
    known: HashMap<NotetypeId, Option<Arc<Notetype>>>,
}

impl<'r, R: ReadRows + ?Sized> Notetypes<'r, R> {
    fn new(reader: &'r R) -> Self {
        Notetypes {
            reader,
            known: HashMap::new(),
        }
    }

    fn get(&mut self, id: NotetypeId) -> Result<Option<Arc<Notetype>>, EngineError> {
        if let Some(known) = self.known.get(&id) {
            return Ok(known.clone());
        }
        let found = self.reader.notetype(id)?.map(Arc::new);
        self.known.insert(id, found.clone());
        Ok(found)
    }
}

/// The notes of the target, a page at a time: listed ids read one by one
/// (deleted ones skipped), all notes by id.
fn each_note(
    reader: &(impl ReadRows + ?Sized),
    target: &ResolvedNotes,
    context: &PlanContext<'_>,
    mut each: impl FnMut(Note) -> Result<(), EngineError>,
) -> Result<u64, EngineError> {
    let progress = context.progress();
    match target {
        ResolvedNotes::Notes(ids) => {
            let total = ids.len() as u64;
            for (done, chunk) in ids.chunks(PAGE).enumerate() {
                context.check_cancel()?;
                if let Some(progress) = progress {
                    progress.report((done * PAGE) as u64, Some(total));
                }
                for &id in chunk {
                    if let Some(note) = reader.note(id)? {
                        each(note)?;
                    }
                }
            }
            if let Some(progress) = progress {
                progress.report(total, Some(total));
            }
            Ok(total)
        }
        ResolvedNotes::AllNotes => {
            let total = reader.row_count(Table::Notes)?;
            let mut read = 0u64;
            let mut after = None;
            loop {
                context.check_cancel()?;
                if let Some(progress) = progress {
                    progress.report(read, Some(total.max(read)));
                }
                let page = reader.notes_after(after, PAGE)?;
                let Some(last) = page.last() else {
                    break;
                };
                after = Some(last.id);
                read += page.len() as u64;
                for note in page {
                    each(note)?;
                }
            }
            if let Some(progress) = progress {
                progress.report(read, Some(read));
            }
            Ok(read)
        }
    }
}

/// The compute phase (F6, F7, F8 on the snapshot).
fn plan(context: &PlanContext<'_>, target: &ResolvedNotes, request: &FindReplace) -> Result<Plan<Planned>, EngineError> {
    let reader = &**context.snapshot();
    let normalize = crate::notes::fields::normalize_enabled(reader)?;
    // F4 step 5 before any note is read.
    let pattern = Arc::new(Pattern::build(request, normalize)?);
    let mut notes = Vec::new();
    let mut first = FirstRefusal::default();
    let considered = if request.within == FindIn::Tags {
        let considered = each_note(reader, target, context, |note| {
            if rewrite_tags(&note.tags, &pattern).is_some() {
                notes.push((note.notetype_id, note.id));
            }
            Ok(())
        })?;
        // F7 step 3: ascending note id, whatever the list's order.
        notes.sort_unstable_by_key(|(_, id)| *id);
        considered
    } else {
        let mut notetypes = Notetypes::new(reader);
        let considered = each_note(reader, target, context, |note| {
            let at = (note.notetype_id, note.id);
            let Some(notetype) = notetypes.get(note.notetype_id)? else {
                first.note(at, Refusal::NotetypeMissing { note: note.id });
                return Ok(());
            };
            match rewrite_fields(&note, &notetype, &request.within, &pattern, normalize) {
                Ok(FieldsRewrite::Changed(_)) => notes.push(at),
                Ok(_) => {}
                Err(refusal) => first.note(at, refusal),
            }
            Ok(())
        })?;
        first.check()?;
        // F6 step 1: ascending note type id, then note id.
        notes.sort_unstable();
        considered
    };
    let mut tables = vec![Table::Notes];
    if request.within != FindIn::Tags {
        tables.extend([Table::Notetypes, Table::Fields, Table::Templates]);
    }
    let preconditions = tables
        .into_iter()
        .map(|table| Precondition::TableUntouched {
            table,
            since: context.seq(),
        })
        .collect();
    Ok(Plan {
        value: Planned {
            pattern,
            normalize,
            notes,
            considered,
        },
        preconditions,
    })
}

/// A row the compute phase planned on that the write transaction finds
/// otherwise: the preconditions rule it out, so this only guards them.
fn drifted(id: NoteId) -> EngineError {
    EngineError::Stale {
        table: Table::Notes,
        key: id.0.to_string(),
    }
}

/// F6 steps 4 and 5 for one note in the write transaction: the live row
/// rewritten again, stamped, and given the cards it now requires. Returns
/// whether it was written.
fn commit_fields_note(
    scope: &mut WriteScope<'_>,
    notetypes: &mut HashMap<NotetypeId, Arc<Notetype>>,
    (mid, id): (NotetypeId, NoteId),
    request: &FindReplace,
    planned: &Planned,
) -> Result<bool, EngineError> {
    let mut note = scope.note(id)?.ok_or_else(|| drifted(id))?;
    if note.notetype_id != mid {
        return Err(drifted(id));
    }
    let notetype = match notetypes.get(&mid) {
        Some(notetype) => Arc::clone(notetype),
        None => {
            let notetype = Arc::new(scope.notetype(mid)?.ok_or_else(|| drifted(id))?);
            notetypes.insert(mid, Arc::clone(&notetype));
            notetype
        }
    };
    let FieldsRewrite::Changed(fields) =
        rewrite_fields(&note, &notetype, &request.within, &planned.pattern, planned.normalize)?
    else {
        return Err(drifted(id));
    };
    note.fields = fields.join("\u{1f}");
    // Step 4: the stored tags re-joined with canonical spacing, nothing else.
    let tags: Vec<&str> = split_text(&note.tags).collect();
    note.tags = join_note_tags(&tags);
    note.sort_field =
        meguri_text::checksum::sort_text_of(&fields, notetype.config.sort_field_idx as usize).into_owned();
    note.checksum = meguri_text::checksum::note_checksum(fields.first().map_or("", String::as_str));
    scope.put(&note)?;
    // Step 5: G7's missing cards, never a deletion. A standard note that
    // already has a card of every template needs none.
    let siblings = scope.cards_of_note(note.id)?;
    let complete = notetype.config.kind != 1
        && (0..notetype.templates.len()).all(|ord| siblings.iter().any(|card| usize::from(card.ord) == ord));
    if !complete {
        let missing = crate::cardgen::plan_note(scope, &notetype, &note, &siblings, None, true)?;
        crate::cardgen::apply_in(scope, &note, &siblings, &missing)?;
    }
    Ok(true)
}

/// T3 for one note's tags, registering nothing: the spellings, or `None`
/// when one of them would register a new row.
fn spell_registered(
    scope: &WriteScope<'_>,
    registered: &mut HashMap<String, String>,
    tags: &[String],
) -> Result<Option<Vec<String>>, EngineError> {
    let mut spelled = Vec::with_capacity(tags.len());
    for tag in tags {
        let normalized = normalize(tag);
        if let Some(spelling) = registered.get(&normalized) {
            spelled.push(spelling.clone());
            continue;
        }
        match spelling_of(scope, &normalized)? {
            Spelling::Registered(name) => {
                registered.insert(normalized, name.clone());
                spelled.push(name);
            }
            Spelling::New(_) => return Ok(None),
        }
    }
    Ok(Some(spelled))
}

/// T4 steps 4 and 5: each spelling once under `unicase`, sorted, joined.
fn stored_tags(spelled: Vec<String>) -> String {
    let mut seen = std::collections::HashSet::new();
    let mut tags: Vec<String> = spelled.into_iter().filter(|tag| seen.insert(folded(tag))).collect();
    tags.sort_by(|a, b| compare_folded(a, b));
    join_note_tags(&tags)
}

/// F7 steps 4 and 5 for one note in the write transaction. Returns whether
/// it was written.
fn commit_tags_note(
    scope: &mut WriteScope<'_>,
    registered: &mut HashMap<String, String>,
    id: NoteId,
    planned: &Planned,
) -> Result<bool, EngineError> {
    let mut note = scope.note(id)?.ok_or_else(|| drifted(id))?;
    let replaced = rewrite_tags(&note.tags, &planned.pattern).ok_or_else(|| drifted(id))?;
    // Step 4: empty results dropped, then T2 and T3 in list order.
    let replaced: Vec<String> = replaced.into_iter().filter(|tag| !tag.is_empty()).collect();
    let tags = match spell_registered(scope, registered, &replaced)? {
        Some(spelled) => stored_tags(spelled),
        None => {
            // A new row: spell and register in list order (T3 can spell a
            // later tag from a row an earlier one registers), and undo the
            // registrations when the note comes out as stored (step 5).
            let savepoint = scope.savepoint()?;
            let mut spelled = Vec::with_capacity(replaced.len());
            for tag in &replaced {
                let normalized = normalize(tag);
                let name = match spelling_of(&*scope, &normalized)? {
                    Spelling::Registered(name) => name,
                    Spelling::New(name) => {
                        scope.put(&new_row(&name))?;
                        name
                    }
                };
                spelled.push(name);
            }
            let tags = stored_tags(spelled);
            if tags == note.tags {
                scope.rollback_to_savepoint(savepoint)?;
                return Ok(false);
            }
            scope.release_savepoint(savepoint)?;
            tags
        }
    };
    if tags == note.tags {
        return Ok(false);
    }
    note.tags = tags;
    scope.put(&note)?;
    Ok(true)
}

/// The write phase: the planned notes rewritten again from their live rows,
/// in the plan's order.
fn commit(context: &mut OpContext<'_, '_>, request: &FindReplace, planned: &Planned) -> Result<BulkCounts, EngineError> {
    let scope = context.scope();
    if crate::notes::fields::normalize_enabled(&*scope)? != planned.normalize {
        return Err(EngineError::Stale {
            table: Table::Config,
            key: "normalize_note_text".to_owned(),
        });
    }
    let mut affected = 0u64;
    if request.within == FindIn::Tags {
        let mut registered = HashMap::new();
        for &(_, id) in &planned.notes {
            if commit_tags_note(scope, &mut registered, id, planned)? {
                affected += 1;
            }
        }
    } else {
        let mut notetypes = HashMap::new();
        for &at in &planned.notes {
            if commit_fields_note(scope, &mut notetypes, at, request, planned)? {
                affected += 1;
            }
        }
    }
    meguri_model::faultpoint!("bulk.find_replace.write_phase")
        .map_err(|error| EngineError::internal(format!("injected write-phase failure: {error}")))?;
    Ok(BulkCounts {
        affected,
        considered: planned.considered,
    })
}

/// F11: Find and Replace on `target`, blocking: the compute phase on a
/// maintenance snapshot, then one write transaction and one undo step
/// ("Find and Replace", or "Update Tag" in Tags, F9).
pub fn run(
    collection: &Collection,
    target: &NoteTarget,
    request: &FindReplace,
    cancel: Arc<CancelToken>,
    progress: ProgressSink,
) -> Result<OpResult<BulkCounts>, EngineError> {
    cancel.check()?;
    let target = target.resolve()?;
    if let ResolvedNotes::Notes(ids) = &target {
        check_ids(ids, "note")?;
    }
    check_request(request)?;
    let step = if request.within == FindIn::Tags {
        UPDATE_TAG
    } else {
        FIND_AND_REPLACE
    };
    let request = Arc::new(request.clone());
    let planning = Arc::clone(&request);
    let (cancel_plan, progress_plan) = (Arc::clone(&cancel), progress.clone());
    plan_and_commit_with(
        collection,
        ReaderRole::Maintenance,
        op_spec(OP_NAME, step, 0, &cancel, &progress),
        move |context| plan(context, &target, &planning),
        move |_, planned| op_spec(OP_NAME, step, planned.notes.len(), &cancel_plan, &progress_plan),
        move |context, planned| commit(context, &request, &planned),
    )
}

/// F11 as a job (`browser/bulk-framework`): returns at once; the result is
/// the job's [`BulkOutcome`].
pub fn start(
    engine: &Engine,
    collection: Collection,
    target: NoteTarget,
    request: FindReplace,
) -> Result<BulkJob, EngineError> {
    super::start(engine, move |context| {
        run(
            &collection,
            &target,
            &request,
            Arc::clone(&context.cancel),
            context.progress.clone(),
        )
        .map(BulkOutcome::of)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(find: &str, replace: &str, regex: bool, ignore_case: bool) -> FindReplace {
        FindReplace {
            find: find.to_owned(),
            replace: replace.to_owned(),
            regex,
            ignore_case,
            within: FindIn::AllFields,
        }
    }

    fn replaced(find: &str, replace: &str, regex: bool, text: &str) -> String {
        Pattern::build(&request(find, replace, regex, true), true)
            .unwrap()
            .replace(text)
            .into_owned()
    }

    #[test]
    fn f4_literal_and_regex_matching() {
        let matches = |find: &str, regex: bool, ignore_case: bool, text: &str| {
            Pattern::build(&request(find, "", regex, ignore_case), true)
                .unwrap()
                .regex
                .find_iter(text)
                .map(|m| m.as_str().to_owned())
                .collect::<Vec<_>>()
        };
        assert_eq!(matches("a.b", false, true, "a.b axb"), ["a.b"]);
        assert_eq!(matches("a.b", true, true, "a.b axb"), ["a.b", "axb"]);
        assert_eq!(matches("CAT", false, false, "cat Cat CAT"), ["CAT"]);
        assert_eq!(matches("cat", false, true, "cat Cat CAT"), ["cat", "Cat", "CAT"]);
        assert_eq!(matches("k", false, true, "K\u{212a}"), ["K", "\u{212a}"]);
        assert!(matches("ss", false, true, "ß").is_empty());
        assert_eq!(matches("(", false, true, "f(x)"), ["("]);
        assert_eq!(matches("", false, true, "ab"), ["", "", ""]);
        let refused = Pattern::build(&request("(", "", true, true), true).unwrap_err();
        assert!(matches!(refused, Refusal::InvalidRegex { .. }), "{refused:?}");
    }

    #[test]
    fn f5_replacement_table() {
        assert_eq!(replaced(r"(\w+)@", "${1} at", true, "Foo@ example"), "Foo at example");
        assert_eq!(replaced(r"(?P<n>\d+)", "${n}kg", true, "12 and 7"), "12kg and 7kg");
        assert_eq!(replaced(r"(\d)", "$1a", true, "a1b2"), "ab");
        assert_eq!(replaced("[aeiou]", "<$0>", true, "kiwi"), "k<i>w<i>");
        assert_eq!(replaced("x", "$$-$ -${", true, "x"), "$-$ -${");
        assert_eq!(replaced(r"(\d)", "<${+1}>", true, "a1"), "a<1>");
        assert_eq!(replaced("^$", "none", true, ""), "none");
        assert_eq!(replaced("", "-", false, "aé"), "-a-é-");
        assert_eq!(replaced("cost", "$5", false, "the cost of it"), "the $5 of it");
    }

    #[test]
    fn f4_nfc_applies_to_fields_only() {
        let decomposed = request("cafe\u{301}", "x", false, true);
        assert!(Pattern::build(&decomposed, true).unwrap().is_match("café"));
        assert!(!Pattern::build(&decomposed, false).unwrap().is_match("café"));
        let tags = FindReplace {
            within: FindIn::Tags,
            ..decomposed
        };
        assert!(!Pattern::build(&tags, true).unwrap().is_match("café"));
    }

    #[test]
    fn joined_comparison_matches_a_join() {
        let fields = |v: &[&str]| v.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
        for (values, stored) in [
            (&["a", "b"][..], "a\u{1f}b"),
            (&["a", ""][..], "a\u{1f}"),
            (&["", ""][..], "\u{1f}"),
            (&["ab"][..], "a\u{1f}b"),
            (&["a", "b"][..], "a\u{1f}bc"),
            (&[""][..], ""),
        ] {
            assert_eq!(
                joined_equals(&fields(values), stored),
                fields(values).join("\u{1f}") == stored,
                "{values:?} {stored:?}"
            );
        }
    }

    #[test]
    fn f7_tags_rows() {
        let tags = |find: &str, regex: bool, replace: &str, stored: &str| {
            let request = FindReplace {
                within: FindIn::Tags,
                ..request(find, replace, regex, true)
            };
            rewrite_tags(stored, &Pattern::build(&request, true).unwrap())
        };
        assert_eq!(tags("^so", true, "x", " also solo "), Some(vec!["also".into(), "xlo".into()]));
        assert_eq!(tags("x$", true, "z", " ax x::x "), Some(vec!["az".into(), "x::z".into()]));
        assert_eq!(tags("", false, "x", " ab "), Some(vec!["xaxbx".into()]));
        assert_eq!(tags("", false, "x", ""), None);
        assert_eq!(tags("drop", false, "", " drop stay "), Some(vec![String::new(), "stay".into()]));
    }
}
