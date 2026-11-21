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
//! [`rewrite_tags`]).
//!
//! The preview (U265, `browser/workspace` V9, F10) is [`preview`]: the same
//! read of the target as the compute phase ([`scan`]), on one snapshot,
//! writing nothing. It counts the notes the write phase would write and
//! lists the changes of the first [`PREVIEW_NOTES`] of them. The run takes
//! that count back as its confirmation (`FindReplace::confirmed`, the
//! pattern of `notetypes/draft-save` D8): the compute phase and the write
//! transaction count again, and a run that would write more notes than
//! were confirmed is refused with `CountChanged`, writing nothing.

use std::borrow::Cow;
use std::cmp::Ordering;
use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::sync::{Arc, LazyLock};

use meguri_model::ids::{NoteId, NotetypeId};
use meguri_model::note::Note;
use meguri_model::notetype::Notetype;
use meguri_storage::{ReadRows, ReaderRole, StorageError, Table, WriteScope};
use meguri_text::fold::{compare_folded, folded};
use regex::{NoExpand, Regex};

use super::{BulkCounts, BulkJob, BulkOutcome, NoteTarget, ResolvedNotes, check_ids, op_spec};
use crate::cancel::CancelToken;
use crate::cardgen::placement::Memo;
use crate::collection::Collection;
use crate::error::EngineError;
use crate::executor::op::{OpContext, OpResult};
use crate::executor::plan_commit::{Plan, PlanContext, plan_and_commit_with};
use crate::executor::preconditions::Precondition;
use crate::lifecycle::Engine;
use crate::progress::ProgressSink;
use crate::tags::registry::{Spelling, TagNames, new_row, spelling_of};
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
    /// `browser/workspace` V9's Whole Words: only the matches with no word
    /// character just before or just after them count.
    pub whole_words: bool,
    pub within: FindIn,
    /// The count of notes that will change as the preview showed it
    /// ([`Preview::changed`]), what the user confirmed (the confirmed-count
    /// precondition, ruled 2026-11-21, the pattern of
    /// `notetypes/draft-save` D8). The run refuses with `CountChanged` when
    /// it would write more notes; fewer or as many proceed. A caller with
    /// no preview passes the count it reads at the call. The preview reads
    /// no confirmation.
    pub confirmed: u64,
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
    /// The confirmed-count precondition (D8's pattern): the run would write
    /// more notes than `FindReplace::confirmed`. `notes` is the count read
    /// where the refusal was made (the compute phase's snapshot or the
    /// write transaction), for the sheet to show again.
    CountChanged { notes: u64 },
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
            Refusal::CountChanged { notes } => {
                write!(f, "{notes} notes would change, more than the confirmed count")
            }
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

/// F4's pattern and F5's replacement, with V9's Whole Words.
#[derive(Clone, Debug)]
pub struct Pattern {
    regex: Regex,
    replace: String,
    expand: bool,
    whole_words: bool,
}

/// One character of the `regex` crate's Unicode `\w` class (V9).
static WORD: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\A\w\z").expect("the word class"));

fn is_word(c: char) -> bool {
    let mut buffer = [0; 4];
    WORD.is_match(c.encode_utf8(&mut buffer))
}

/// V9: a match counts under Whole Words when the character just before it
/// and the one just after it, where there are any, are not word characters.
fn whole_word(text: &str, start: usize, end: usize) -> bool {
    !text[..start].chars().next_back().is_some_and(is_word) && !text[end..].chars().next().is_some_and(is_word)
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
            whole_words: request.whole_words,
        })
    }

    /// Whether the pattern has at least one match in `text` (an empty
    /// pattern matches every text, the empty one included); under Whole
    /// Words, one that V9 keeps.
    pub fn is_match(&self, text: &str) -> bool {
        if self.whole_words {
            self.regex
                .find_iter(text)
                .any(|found| whole_word(text, found.start(), found.end()))
        } else {
            self.regex.is_match(text)
        }
    }

    /// F5: every match replaced, left to right, without overlap. Under
    /// Whole Words the matches V9 drops stay as they stand, and the search
    /// goes on after them as it does after a replaced one.
    pub fn replace<'t>(&self, text: &'t str) -> Cow<'t, str> {
        if self.whole_words {
            return self.replace_whole_words(text);
        }
        if self.expand {
            self.regex.replace_all(text, self.replace.as_str())
        } else {
            self.regex.replace_all(text, NoExpand(&self.replace))
        }
    }

    fn replace_whole_words<'t>(&self, text: &'t str) -> Cow<'t, str> {
        let mut out = String::new();
        let mut last = 0;
        let mut replaced = false;
        for captures in self.regex.captures_iter(text) {
            let Some(found) = captures.get(0) else { continue };
            if !whole_word(text, found.start(), found.end()) {
                continue;
            }
            replaced = true;
            out.push_str(&text[last..found.start()]);
            if self.expand {
                captures.expand(&self.replace, &mut out);
            } else {
                out.push_str(&self.replace);
            }
            last = found.end();
        }
        if !replaced {
            return Cow::Borrowed(text);
        }
        out.push_str(&text[last..]);
        Cow::Owned(out)
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

/// How a read of the target paces itself: the cancel check between pages,
/// and where its progress goes (none inside the write transaction, K3).
struct Pace<'p> {
    check: &'p dyn Fn() -> Result<(), EngineError>,
    progress: Option<&'p ProgressSink>,
}

impl Pace<'_> {
    fn report(&self, read: u64, total: u64) {
        if let Some(progress) = self.progress {
            progress.report(read, Some(total));
        }
    }
}

/// The notes of the target, a page at a time: listed ids read one by one
/// (deleted ones skipped), in the list's order or, with `ascending`, in
/// ascending note id; all notes by id.
fn each_note(
    reader: &(impl ReadRows + ?Sized),
    target: &ResolvedNotes,
    ascending: bool,
    pace: &Pace<'_>,
    mut each: impl FnMut(Note) -> Result<(), EngineError>,
) -> Result<u64, EngineError> {
    match target {
        ResolvedNotes::Notes(listed) => {
            let total = listed.len() as u64;
            let sorted;
            let ids: &[NoteId] = if ascending {
                let mut ids = listed.clone();
                ids.sort_unstable();
                sorted = ids;
                &sorted
            } else {
                listed
            };
            for (done, chunk) in ids.chunks(PAGE).enumerate() {
                (pace.check)()?;
                pace.report((done * PAGE) as u64, total);
                let mut page = reader.notes_with_ids(chunk)?;
                if ascending {
                    page.sort_unstable_by_key(|note| note.id);
                }
                for note in page {
                    each(note)?;
                }
            }
            pace.report(total, total);
            Ok(total)
        }
        ResolvedNotes::AllNotes => {
            let total = reader.row_count(Table::Notes)?;
            let mut read = 0u64;
            let mut after = None;
            loop {
                (pace.check)()?;
                pace.report(read, total.max(read));
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
            pace.report(read, read);
            Ok(read)
        }
    }
}

/// A tag name ordered and compared as the `tags` table's key (`unicase`).
#[derive(Debug)]
struct TagKey(String);

impl PartialEq for TagKey {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

impl Eq for TagKey {}

impl PartialOrd for TagKey {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for TagKey {
    fn cmp(&self, other: &Self) -> Ordering {
        compare_folded(&self.0, &other.0)
    }
}

/// The tag registry as the write phase would leave it so far (F7 step 4,
/// T3): the stored rows, and in memory the rows the notes before this one
/// would register. So a read spells every note's tags as the write phase
/// will, registering nothing.
struct Registry<'r, R: ?Sized> {
    rows: &'r R,
    added: BTreeMap<TagKey, String>,
}

impl<'r, R: ReadRows + ?Sized> Registry<'r, R> {
    fn new(rows: &'r R) -> Self {
        Registry {
            rows,
            added: BTreeMap::new(),
        }
    }

    /// F7 steps 4 and 5 for one matched note as `commit_tags_note` runs
    /// them: the tags text the write phase would store, or `None` when it
    /// is the stored text (nothing written, nothing registered). `replaced`
    /// is [`rewrite_tags`]'s result.
    fn stored_after(&mut self, stored: &str, replaced: Vec<String>) -> Result<Option<String>, EngineError> {
        let mut registered = Vec::new();
        let mut spelled = Vec::with_capacity(replaced.len());
        for tag in replaced.iter().filter(|tag| !tag.is_empty()) {
            let normalized = normalize(tag);
            let name = match spelling_of(&*self, &normalized)? {
                Spelling::Registered(name) => name,
                Spelling::New(name) => {
                    // A later tag of this note spells from it too (T3).
                    self.added.insert(TagKey(name.clone()), name.clone());
                    registered.push(name.clone());
                    name
                }
            };
            spelled.push(name);
        }
        let tags = stored_tags(spelled);
        if tags == stored {
            // The write phase rolls those registrations back (step 5).
            for name in registered {
                self.added.remove(&TagKey(name));
            }
            return Ok(None);
        }
        Ok(Some(tags))
    }
}

impl<R: ReadRows + ?Sized> TagNames for Registry<'_, R> {
    fn registered_name(&self, name: &str) -> Result<Option<String>, StorageError> {
        match self.added.get(&TagKey(name.to_owned())) {
            Some(added) => Ok(Some(added.clone())),
            None => self.rows.registered_name(name),
        }
    }

    fn first_name_from(&self, from: &str) -> Result<Option<String>, StorageError> {
        let stored = self.rows.first_name_from(from)?;
        let added = self.added.range(TagKey(from.to_owned())..).next().map(|(_, name)| name);
        Ok(match (stored, added) {
            (Some(stored), Some(added)) if compare_folded(added, &stored) == Ordering::Less => Some(added.clone()),
            (Some(stored), _) => Some(stored),
            (None, added) => added.cloned(),
        })
    }
}

/// A note the run changes, as a read of the target finds it, with what the
/// write phase would store.
enum Change<'n> {
    /// F6: the stored note and its note type, and the values to store.
    Fields {
        note: &'n Note,
        notetype: &'n Notetype,
        fields: &'n [String],
    },
    /// F7: the stored note and the tags text to store.
    Tags { note: &'n Note, tags: &'n str },
}

/// What a read of the target found.
struct Scanned {
    considered: u64,
    /// The notes the write phase would write (F6 step 6, F7 step 5): the
    /// preview's count and the confirmed count's measure.
    changed: u64,
    /// The notes the write phase visits: for fields those it changes, in F6
    /// step 1's order; for tags those that match, ascending.
    notes: Vec<(NotetypeId, NoteId)>,
}

/// The read of the target the compute phase, the preview and the write
/// transaction's count share (F6, F7, F8): every note read on `reader`,
/// F6 step 7's refusals, and each note the run changes handed to `each`
/// with what the write phase would store. Writes nothing.
fn scan<R: ReadRows + ?Sized>(
    reader: &R,
    target: &ResolvedNotes,
    request: &FindReplace,
    pattern: &Pattern,
    normalize: bool,
    pace: &Pace<'_>,
    mut each: impl FnMut(Change<'_>) -> Result<(), EngineError>,
) -> Result<Scanned, EngineError> {
    let mut notes = Vec::new();
    let mut changed = 0u64;
    if request.within == FindIn::Tags {
        // F7 step 3: ascending note id, whatever the list's order, so that
        // each note spells from the rows the notes before it register.
        let mut registry = Registry::new(reader);
        let considered = each_note(reader, target, true, pace, |note| {
            let Some(replaced) = rewrite_tags(&note.tags, pattern) else {
                return Ok(());
            };
            notes.push((note.notetype_id, note.id));
            if let Some(tags) = registry.stored_after(&note.tags, replaced)? {
                changed += 1;
                each(Change::Tags { note: &note, tags: &tags })?;
            }
            Ok(())
        })?;
        return Ok(Scanned {
            considered,
            changed,
            notes,
        });
    }
    let mut first = FirstRefusal::default();
    let mut notetypes = Notetypes::new(reader);
    let considered = each_note(reader, target, false, pace, |note| {
        let at = (note.notetype_id, note.id);
        let Some(notetype) = notetypes.get(note.notetype_id)? else {
            first.note(at, Refusal::NotetypeMissing { note: note.id });
            return Ok(());
        };
        match rewrite_fields(&note, &notetype, &request.within, pattern, normalize) {
            Ok(FieldsRewrite::Changed(fields)) => {
                notes.push(at);
                changed += 1;
                each(Change::Fields {
                    note: &note,
                    notetype: &notetype,
                    fields: &fields,
                })?;
            }
            Ok(_) => {}
            Err(refusal) => first.note(at, refusal),
        }
        Ok(())
    })?;
    first.check()?;
    // F6 step 1: ascending note type id, then note id.
    notes.sort_unstable();
    Ok(Scanned {
        considered,
        changed,
        notes,
    })
}

/// The compute phase (F6, F7, F8 on the snapshot), then the confirmed
/// count's first check: an out-of-date confirmation is refused before the
/// write transaction is asked for.
fn plan(context: &PlanContext<'_>, target: &ResolvedNotes, request: &FindReplace) -> Result<Plan<Planned>, EngineError> {
    let reader = &**context.snapshot();
    let normalize = crate::notes::fields::normalize_enabled(reader)?;
    // F4 step 5 before any note is read.
    let pattern = Arc::new(Pattern::build(request, normalize)?);
    let check = || context.check_cancel();
    let pace = Pace {
        check: &check,
        progress: context.progress(),
    };
    let scanned = scan(reader, target, request, &pattern, normalize, &pace, |_| Ok(()))?;
    if scanned.changed > request.confirmed {
        return Err(Refusal::CountChanged {
            notes: scanned.changed,
        }
        .into());
    }
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
            notes: scanned.notes,
            considered: scanned.considered,
        },
        preconditions,
    })
}

/// The confirmed count's second check, on the rows of the write transaction
/// before its first write (the shape of `notetypes/structure` `check_live`;
/// the preconditions, and so `Stale`, were checked before it).
///
/// The write phase writes only planned notes, so it writes at most
/// `planned.notes.len()`: within the confirmation, nothing can exceed it.
/// Otherwise the planned notes are read again on the live rows and counted
/// as the write phase will write them. No other note of the target can
/// change: the preconditions hold the notes, note types, fields and
/// templates as the compute phase read them, and a note that does not
/// match never changes, whatever the tag registry holds. For fields the
/// planned notes are exactly those counted on the snapshot, so only a Tags
/// run, whose spelling reads the registry the preconditions do not hold,
/// counts here.
fn check_live(scope: &WriteScope<'_>, request: &FindReplace, planned: &Planned) -> Result<(), EngineError> {
    if planned.notes.len() as u64 <= request.confirmed {
        return Ok(());
    }
    let ids = ResolvedNotes::Notes(planned.notes.iter().map(|(_, id)| *id).collect());
    // Past the cancel point and inside the transaction: no cancel, no
    // progress (K3).
    let pace = Pace {
        check: &|| Ok(()),
        progress: None,
    };
    let live = scan(scope, &ids, request, &planned.pattern, planned.normalize, &pace, |_| Ok(()))?.changed;
    if live > request.confirmed {
        return Err(Refusal::CountChanged { notes: live }.into());
    }
    Ok(())
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
#[allow(clippy::too_many_arguments)]
fn commit_fields_note(
    scope: &mut WriteScope<'_>,
    notetypes: &mut HashMap<NotetypeId, Arc<(Notetype, Vec<meguri_template::ParsedTemplate>)>>,
    memo: &Memo,
    mut note: Note,
    mid: NotetypeId,
    siblings: Vec<meguri_model::card::Card>,
    request: &FindReplace,
    planned: &Planned,
) -> Result<bool, EngineError> {
    let id = note.id;
    if note.notetype_id != mid {
        return Err(drifted(id));
    }
    let known = match notetypes.get(&mid) {
        Some(known) => Arc::clone(known),
        None => {
            let notetype = scope.notetype(mid)?.ok_or_else(|| drifted(id))?;
            let fronts = crate::cardgen::normal::fronts(&notetype);
            let known = Arc::new((notetype, fronts));
            notetypes.insert(mid, Arc::clone(&known));
            known
        }
    };
    let (notetype, fronts) = (&known.0, &known.1);
    let FieldsRewrite::Changed(fields) =
        rewrite_fields(&note, notetype, &request.within, &planned.pattern, planned.normalize)?
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
    // already has a card of every template needs none. `siblings` are its
    // live cards: only this note's own generation adds to them.
    let complete = notetype.config.kind != 1
        && (0..notetype.templates.len()).all(|ord| siblings.iter().any(|card| usize::from(card.ord) == ord));
    if !complete {
        let missing = crate::cardgen::plan_note_with(scope, memo, notetype, fronts, &note, &siblings, None, true)?;
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
    mut note: Note,
    planned: &Planned,
) -> Result<bool, EngineError> {
    let id = note.id;
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
    // The key shapes the pattern and the saved fields; Tags reads neither.
    if request.within != FindIn::Tags && crate::notes::fields::normalize_enabled(&*scope)? != planned.normalize {
        return Err(EngineError::Stale {
            table: Table::Config,
            key: "normalize_note_text".to_owned(),
        });
    }
    // The confirmed count, counted again ahead of the first write; `Stale`
    // (the preconditions, the key above) comes first.
    check_live(scope, request, planned)?;
    let mut affected = 0u64;
    let mut registered = HashMap::new();
    let mut notetypes = HashMap::new();
    // Placement's deck and last-deck reads: this op writes neither.
    let memo = Memo::default();
    // A page of live rows at a time, each written in the plan's order before
    // the next page is read.
    for chunk in planned.notes.chunks(PAGE) {
        let ids: Vec<NoteId> = chunk.iter().map(|(_, id)| *id).collect();
        let mut live: HashMap<NoteId, Note> = scope
            .notes_with_ids(&ids)?
            .into_iter()
            .map(|note| (note.id, note))
            .collect();
        let mut cards: HashMap<NoteId, Vec<meguri_model::card::Card>> = HashMap::new();
        if request.within != FindIn::Tags {
            for card in scope.cards_of_notes_with_ids(&ids)? {
                cards.entry(card.note_id).or_default().push(card);
            }
        }
        for &(mid, id) in chunk {
            let note = live.remove(&id).ok_or_else(|| drifted(id))?;
            let written = if request.within == FindIn::Tags {
                commit_tags_note(scope, &mut registered, note, planned)?
            } else {
                let siblings = cards.remove(&id).unwrap_or_default();
                commit_fields_note(scope, &mut notetypes, &memo, note, mid, siblings, request, planned)?
            };
            if written {
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
    let target = target.resolve(collection)?;
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

/// The notes whose changes a preview lists (`browser/workspace` V9, V17).
pub const PREVIEW_NOTES: usize = 100;

/// What a Find and Replace would do (`browser/workspace` V9, F10), read
/// without writing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Preview {
    /// The notes the run would write (F6 step 6, F7 step 5: a match whose
    /// replacement gives back the stored value does not count): the
    /// sheet's "c of k notes will change", and what the run takes back as
    /// `FindReplace::confirmed`.
    pub changed: u64,
    /// The run's `considered` (F8): the listed ids, deleted ones included;
    /// the notes read for the all-notes target.
    pub considered: u64,
    /// The changes of the first [`PREVIEW_NOTES`] changed notes in result
    /// order (the list's order; ascending note id for the all-notes
    /// target), each note's in field order.
    pub entries: Vec<PreviewEntry>,
}

/// One changed (note, field) pair; Tags count as one field (V9).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreviewEntry {
    pub note: NoteId,
    /// The note's Sort Field as Browse shows it (`browser/columns` C7.1),
    /// before the change; empty when its note type is missing.
    pub sort_field: String,
    /// The changed field's name (the sheet shows it only with All Fields);
    /// `None` for Tags.
    pub field: Option<String>,
    /// The old and the new value as one line of text (a field's stored
    /// value with HTML removed by C7.1's rule; tags as C7.10 shows them),
    /// marked with the character diff between the two: the shortest edit
    /// script, each run of changes listing its removed text before its
    /// inserted text.
    pub diff: Vec<DiffSpan>,
}

impl PreviewEntry {
    /// The old text: what is kept and what is removed.
    pub fn old_text(&self) -> String {
        self.text(Edit::Removed)
    }

    /// The new text: what is kept and what is inserted.
    pub fn new_text(&self) -> String {
        self.text(Edit::Inserted)
    }

    fn text(&self, with: Edit) -> String {
        self.diff
            .iter()
            .filter(|span| span.edit == Edit::Kept || span.edit == with)
            .map(|span| span.text.as_str())
            .collect()
    }
}

/// What a span of a preview entry's text is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Edit {
    Kept,
    /// Only in the old text (crossed out in red).
    Removed,
    /// Only in the new text (in the accent colour).
    Inserted,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiffSpan {
    pub edit: Edit,
    pub text: String,
}

/// The preview of a Find and Replace on `target` (V9, F10): the same read
/// of the notes as the run's compute phase, on one snapshot of the
/// maintenance reader (`browser/bulk-framework` K3), writing nothing.
/// Refuses as the run would (F11's refusals in its order), so an invalid
/// pattern is F10's error. `cancel` is checked between pages: the sheet
/// cancels a preview the next keystroke supersedes. `request.confirmed` is
/// not read.
pub fn preview(
    collection: &Collection,
    target: &NoteTarget,
    request: &FindReplace,
    cancel: &CancelToken,
    progress: &ProgressSink,
) -> Result<Preview, EngineError> {
    cancel.check()?;
    let target = target.resolve(collection)?;
    if let ResolvedNotes::Notes(ids) = &target {
        check_ids(ids, "note")?;
    }
    check_request(request)?;
    let reader = collection.reader(ReaderRole::Maintenance)?;
    let snapshot = reader.snapshot()?;
    preview_in(&*snapshot, &target, request, cancel, progress)
}

/// [`preview`] on a snapshot the caller holds, for a resolved target that
/// passed `check_ids` and a request that passed [`check_request`].
pub fn preview_in(
    reader: &(impl ReadRows + ?Sized),
    target: &ResolvedNotes,
    request: &FindReplace,
    cancel: &CancelToken,
    progress: &ProgressSink,
) -> Result<Preview, EngineError> {
    let normalize = crate::notes::fields::normalize_enabled(reader)?;
    let pattern = Pattern::build(request, normalize)?;
    let mut listed = Listed::new(target, reader);
    let check = || cancel.check();
    let pace = Pace {
        check: &check,
        progress: Some(progress),
    };
    let scanned = scan(reader, target, request, &pattern, normalize, &pace, |change| listed.offer(change))?;
    // A cancel during the last page's reports.
    cancel.check()?;
    Ok(Preview {
        changed: scanned.changed,
        considered: scanned.considered,
        entries: listed.finish(cancel)?,
    })
}

/// A preview entry before its diff: the texts it compares.
struct Pending {
    note: NoteId,
    sort_field: String,
    field: Option<String>,
    old: String,
    new: String,
}

/// The preview's list: the entries of the first [`PREVIEW_NOTES`] changed
/// notes by result position, whatever order the read finds them in. Their
/// diffs are built once the read has ended, for the notes that stay.
struct Listed<'r, R: ReadRows + ?Sized> {
    /// A listed note's position in the list; `None` for the all-notes
    /// target, whose position is the note id.
    positions: Option<HashMap<NoteId, usize>>,
    notetypes: Notetypes<'r, R>,
    entries: BTreeMap<i64, Vec<Pending>>,
}

impl<'r, R: ReadRows + ?Sized> Listed<'r, R> {
    fn new(target: &ResolvedNotes, reader: &'r R) -> Self {
        let positions = match target {
            ResolvedNotes::Notes(ids) => Some(ids.iter().enumerate().map(|(at, id)| (*id, at)).collect()),
            ResolvedNotes::AllNotes => None,
        };
        Listed {
            positions,
            notetypes: Notetypes::new(reader),
            entries: BTreeMap::new(),
        }
    }

    fn offer(&mut self, change: Change<'_>) -> Result<(), EngineError> {
        let note = match &change {
            Change::Fields { note, .. } | Change::Tags { note, .. } => *note,
        };
        let at = match &self.positions {
            Some(positions) => positions.get(&note.id).map_or(i64::MAX, |at| *at as i64),
            None => note.id.0,
        };
        if self.entries.len() >= PREVIEW_NOTES && self.entries.last_key_value().is_some_and(|(last, _)| at >= *last) {
            return Ok(());
        }
        let entries = match change {
            Change::Fields { note, notetype, fields } => field_entries(note, notetype, fields),
            Change::Tags { note, tags } => {
                // Display only: the Tags run reads no note type, so one that
                // cannot be read leaves the Sort Field empty, not the preview
                // refused.
                let notetype = self.notetypes.get(note.notetype_id).ok().flatten();
                vec![Pending {
                    note: note.id,
                    sort_field: notetype.map_or_else(String::new, |notetype| sort_field_text(note, &notetype)),
                    field: None,
                    old: tags_line(&note.tags),
                    new: tags_line(tags),
                }]
            }
        };
        self.entries.insert(at, entries);
        if self.entries.len() > PREVIEW_NOTES {
            self.entries.pop_last();
        }
        Ok(())
    }

    /// The listed entries with their diffs, in result order, then field
    /// order; `cancel` is checked before each diff.
    fn finish(self, cancel: &CancelToken) -> Result<Vec<PreviewEntry>, EngineError> {
        let mut out = Vec::new();
        for pending in self.entries.into_values().flatten() {
            cancel.check()?;
            out.push(PreviewEntry {
                note: pending.note,
                sort_field: pending.sort_field,
                field: pending.field,
                diff: char_diff(&pending.old, &pending.new),
            });
        }
        Ok(out)
    }
}

/// C7.1: the stored value at the note type's sort field, as one line of
/// text with media names kept; empty when the note has no value there.
fn sort_field_text(note: &Note, notetype: &Notetype) -> String {
    let at = notetype.config.sort_field_idx as usize;
    note.field_values()
        .nth(at)
        .map_or_else(String::new, meguri_text::html::text_line_keep_media)
}

/// C7.10: the tags in stored order, joined by one U+0020.
fn tags_line(tags: &str) -> String {
    split_text(tags).collect::<Vec<_>>().join(" ")
}

/// V9's entries of a note whose fields change: one per field whose stored
/// value changes (normalization included), in field order.
fn field_entries(note: &Note, notetype: &Notetype, fields: &[String]) -> Vec<Pending> {
    let sort_field = sort_field_text(note, notetype);
    note.field_values()
        .zip(fields)
        .enumerate()
        .filter(|(_, (old, new))| *old != new.as_str())
        .map(|(ord, (old, new))| Pending {
            note: note.id,
            sort_field: sort_field.clone(),
            field: notetype.fields.get(ord).map(|field| field.name.clone()),
            old: meguri_text::html::text_line_keep_media(old),
            new: meguri_text::html::text_line_keep_media(new),
        })
        .collect()
}

/// The search for the shortest edit script gives up above this many
/// differences, and above [`MAX_STEPS`] steps (the characters compared
/// times the differences tried). Past either limit, between the common
/// start and end, the old text is shown removed and the new one inserted.
const MAX_EDITS: usize = 500;
const MAX_STEPS: usize = 50_000_000;

/// V9's character diff from `old` to `new`.
fn char_diff(old: &str, new: &str) -> Vec<DiffSpan> {
    let old: Vec<char> = old.chars().collect();
    let new: Vec<char> = new.chars().collect();
    let head = old.iter().zip(&new).take_while(|(a, b)| a == b).count();
    let tail = old[head..]
        .iter()
        .rev()
        .zip(new[head..].iter().rev())
        .take_while(|(a, b)| a == b)
        .count();
    let (a, b) = (&old[head..old.len() - tail], &new[head..new.len() - tail]);
    let mut edits = vec![(Edit::Kept, &old[..head])];
    match shortest_edits(a, b) {
        Some(steps) => edits.extend(steps),
        None => edits.extend([(Edit::Removed, a), (Edit::Inserted, b)]),
    }
    edits.push((Edit::Kept, &old[old.len() - tail..]));
    spans(edits)
}

/// Joins runs of one kind, lists each run of changes' removals before its
/// insertions, and drops empty pieces.
fn spans(edits: Vec<(Edit, &[char])>) -> Vec<DiffSpan> {
    let mut out: Vec<DiffSpan> = Vec::new();
    let mut removed = String::new();
    let mut inserted = String::new();
    let flush = |out: &mut Vec<DiffSpan>, removed: &mut String, inserted: &mut String| {
        for (edit, text) in [(Edit::Removed, removed), (Edit::Inserted, inserted)] {
            if !text.is_empty() {
                out.push(DiffSpan {
                    edit,
                    text: std::mem::take(text),
                });
            }
        }
    };
    for (edit, chars) in edits {
        if chars.is_empty() {
            continue;
        }
        match edit {
            Edit::Removed => removed.extend(chars),
            Edit::Inserted => inserted.extend(chars),
            Edit::Kept => {
                flush(&mut out, &mut removed, &mut inserted);
                match out.last_mut() {
                    Some(last) if last.edit == Edit::Kept => last.text.extend(chars),
                    _ => out.push(DiffSpan {
                        edit: Edit::Kept,
                        text: chars.iter().collect(),
                    }),
                }
            }
        }
    }
    flush(&mut out, &mut removed, &mut inserted);
    out
}

/// The shortest edit script from `a` to `b` (the greedy forward search
/// round by round, one more difference each round, then a walk back), as
/// runs of kept, removed and inserted characters in text order; `None` when
/// it needs more removals and insertions than [`MAX_EDITS`] or, for long
/// texts, than [`MAX_STEPS`] allows.
fn shortest_edits<'c>(a: &'c [char], b: &'c [char]) -> Option<Vec<(Edit, &'c [char])>> {
    let (n, m) = (a.len() as isize, b.len() as isize);
    let rounds = MAX_STEPS / (a.len() + b.len()).max(1);
    let limit = (n + m).min(MAX_EDITS.min(rounds) as isize);
    // `furthest[k + offset]`: the largest x reached where x - y = k.
    let offset = limit + 1;
    let mut furthest = vec![0isize; (2 * limit + 3) as usize];
    // Before round d: the reach for k from -d-1 to d+1 as round d - 1 left it.
    let mut rounds: Vec<Vec<isize>> = Vec::new();
    let mut found = None;
    'search: for d in 0..=limit {
        rounds.push(furthest[(offset - d - 1) as usize..=(offset + d + 1) as usize].to_vec());
        for k in (-d..=d).step_by(2) {
            let at = |k: isize| furthest[(k + offset) as usize];
            let down = k == -d || (k != d && at(k - 1) < at(k + 1));
            let mut x = if down { at(k + 1) } else { at(k - 1) + 1 };
            let mut y = x - k;
            while x < n && y < m && a[x as usize] == b[y as usize] {
                x += 1;
                y += 1;
            }
            furthest[(k + offset) as usize] = x;
            if x >= n && y >= m {
                found = Some(d);
                break 'search;
            }
        }
    }
    let found = found?;
    let mut steps: Vec<(Edit, &[char])> = Vec::new();
    let (mut x, mut y) = (n, m);
    for d in (0..=found).rev() {
        let before = &rounds[d as usize];
        let at = |k: isize| before[(k + d + 1) as usize];
        let k = x - y;
        let down = k == -d || (k != d && at(k - 1) < at(k + 1));
        let prev_k = if down { k + 1 } else { k - 1 };
        let prev_x = at(prev_k);
        let prev_y = prev_x - prev_k;
        // The run of equal characters this round ended with: kept.
        let run = (x - prev_x).min(y - prev_y).max(0);
        if run > 0 {
            steps.push((Edit::Kept, &a[(x - run) as usize..x as usize]));
            x -= run;
            y -= run;
        }
        if d > 0 {
            if down {
                steps.push((Edit::Inserted, &b[prev_y as usize..y as usize]));
            } else {
                steps.push((Edit::Removed, &a[prev_x as usize..x as usize]));
            }
            x = prev_x;
            y = prev_y;
        }
    }
    steps.reverse();
    Some(steps)
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
            whole_words: false,
            within: FindIn::AllFields,
            confirmed: 0,
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

    fn whole(find: &str, replace: &str, regex: bool, text: &str) -> (bool, String) {
        let pattern = Pattern::build(
            &FindReplace {
                whole_words: true,
                ..request(find, replace, regex, true)
            },
            true,
        )
        .unwrap();
        (pattern.is_match(text), pattern.replace(text).into_owned())
    }

    /// V9's Whole Words: a match counts only with no word character (`\w`,
    /// Unicode) just before and just after it, whatever its own first and
    /// last characters; dropped matches stay as they stand.
    #[test]
    fn v9_whole_words() {
        assert_eq!(whole("pim", "rok", false, "pim pims spim pim."), (true, "rok pims spim rok.".into()));
        assert_eq!(whole("C++", "x", false, "C++x"), (false, "C++x".into()), "V9's example");
        assert_eq!(whole("C++", "x", false, "(C++) C++"), (true, "(x) x".into()));
        assert_eq!(whole("é", "e", false, "é éa aé"), (true, "e éa aé".into()), "Unicode word characters");
        assert_eq!(whole("_x", "y", false, "a_x _x"), (true, "a_x y".into()), "`_` is a word character");
        assert_eq!(whole(r"(\d+)", "<$1>", true, "12 a3 4b 5"), (true, "<12> a3 4b <5>".into()));
        assert_eq!(whole("a+", "-", true, "baa aa"), (true, "baa -".into()), "a dropped match is not retried");
        assert_eq!(
            whole("pim|pims", "-", true, "pims"),
            (false, "pims".into()),
            "the first match `pim` is dropped and `pims` is not tried at its start"
        );
        assert_eq!(whole("", "|", false, "ab c"), (false, "ab c".into()), "every empty match touches a word");
        assert_eq!(whole("", "|", false, "- -"), (true, "|-| |-|".into()), "empty matches between non-word characters");
        assert_eq!(whole("cost", "$5", false, "cost"), (true, "$5".into()), "literal `$` kept");
    }

    fn diff(old: &str, new: &str) -> Vec<(Edit, String)> {
        char_diff(old, new).into_iter().map(|span| (span.edit, span.text)).collect()
    }

    /// V9's character diff: the shortest edit script, removals before
    /// insertions in each run of changes, adjacent pieces of one kind
    /// joined.
    #[test]
    fn v9_character_diff() {
        use Edit::{Inserted, Kept, Removed};
        let s = |text: &str| text.to_owned();
        assert_eq!(diff("the cost of it", "the $5 of it"), [
            (Kept, s("the ")),
            (Removed, s("cost")),
            (Inserted, s("$5")),
            (Kept, s(" of it")),
        ]);
        assert_eq!(diff("abc", "abc"), [(Kept, s("abc"))]);
        assert_eq!(diff("", "new"), [(Inserted, s("new"))]);
        assert_eq!(diff("old", ""), [(Removed, s("old"))]);
        assert_eq!(diff("", ""), []);
        assert_eq!(diff("kiwi", "k<i>w<i>"), [
            (Kept, s("k")),
            (Inserted, s("<")),
            (Kept, s("i")),
            (Inserted, s(">")),
            (Kept, s("w")),
            (Inserted, s("<")),
            (Kept, s("i")),
            (Inserted, s(">")),
        ]);
        assert_eq!(diff("abcabba", "cbabac").iter().filter(|(e, _)| *e != Kept).map(|(_, t)| t.chars().count()).sum::<usize>(), 5, "the shortest script has five edits");
        // Every diff spells both texts back.
        for (old, new) in [("abcabba", "cbabac"), ("a😀b", "😀"), ("lobe", "lode"), ("x", "yxy")] {
            let spans = char_diff(old, new);
            let entry = PreviewEntry {
                note: NoteId(1),
                sort_field: String::new(),
                field: None,
                diff: spans,
            };
            assert_eq!((entry.old_text(), entry.new_text()), (old.to_owned(), new.to_owned()));
        }
        // Too many differences: one removal and one insertion between the
        // common start and end.
        let old = "a".repeat(MAX_EDITS);
        let new = "b".repeat(MAX_EDITS);
        let long = diff(&format!("<{old}>"), &format!("<{new}>"));
        assert_eq!(long.len(), 4, "{long:?}");
        assert_eq!(long[1], (Removed, old.clone()));
        assert_eq!(long[2], (Inserted, new.clone()));
        // A long text with few changes keeps what it shares.
        let middle = "m".repeat(30_000);
        assert_eq!(diff(&format!("a{middle}b"), &format!("x{middle}y")), [
            (Removed, s("a")),
            (Inserted, s("x")),
            (Kept, middle.clone()),
            (Removed, s("b")),
            (Inserted, s("y")),
        ]);
    }
}
